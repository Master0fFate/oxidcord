use std::collections::BTreeMap;

use plugin_sdk::types::FetchRequest;
use plugin_sdk::{
	OutgoingMessage, Plugin, PluginExt, SendDecision, export_plugin, host, log, now_ms, storage, timer,
};
use regex_lite::{Regex, RegexBuilder};
use serde::Deserialize;

const RULES_URL: &str = "https://raw.githubusercontent.com/ClearURLs/Rules/master/data.min.json";
const RULES_KEY: &str = "rules";
const FETCHED_KEY: &str = "fetched-ms";
const REFRESH_TAG: &str = "refresh";
const FIRST_REFRESH_MS: u32 = 3_000;
const CHECK_EVERY_MS: u32 = 6 * 60 * 60 * 1000;
const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;
const REGEX_SIZE_LIMIT: usize = 1 << 20;
const MAX_REDIRECTS: usize = 3;
const MAX_CONTENT_BYTES: usize = 4000;
const TRAILING_PUNCTUATION: &str = ".,:;\"'>)|]";
const FALLBACK_RULES: &str = r#"{"providers":{"globalRules":{"urlPattern":"^https?:\\/\\/","rules":["(?:%3F)?utm(?:_[a-z_]*)?","(?:%3F)?ga_[a-z_]+","(?:%3F)?yclid","(?:%3F)?fb_(?:source|ref)","(?:%3F)?fbclid","(?:%3F)?gclid","(?:%3F)?dclid","(?:%3F)?msclkid","(?:%3F)?twclid","(?:%3F)?mkt_tok","(?:%3F)?_ga","(?:%3F)?_gl","(?:%3F)?mc_(?:eid|cid|tc)","(?:%3F)?igshid","(?:%3F)?srsltid"]}}}"#;

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Source {
	url_pattern: String,
	rules: Vec<String>,
	referral_marketing: Vec<String>,
	raw_rules: Vec<String>,
	exceptions: Vec<String>,
	redirections: Vec<String>,
}

#[derive(Deserialize)]
struct Document {
	providers: BTreeMap<String, Source>,
}

struct Prepared {
	exceptions: Vec<Regex>,
	params: Option<Regex>,
	raw_rules: Vec<Regex>,
	redirections: Vec<Regex>,
}

struct Provider {
	pattern: Regex,
	source: Source,
	prepared: Option<Prepared>,
}

#[derive(Default)]
struct ClearUrls {
	providers: Vec<Provider>,
}

fn build(pattern: &str) -> Option<Regex> {
	RegexBuilder::new(pattern)
		.case_insensitive(true)
		.size_limit(REGEX_SIZE_LIMIT)
		.build()
		.ok()
}

fn build_all(patterns: &[String]) -> Vec<Regex> {
	patterns.iter().filter_map(|pattern| build(pattern)).collect()
}

fn parse_providers(bytes: &[u8]) -> Option<Vec<Provider>> {
	let document: Document = serde_json::from_slice(bytes).ok()?;
	let providers: Vec<Provider> = document
		.providers
		.into_values()
		.filter_map(|source| {
			Some(Provider {
				pattern: build(&source.url_pattern)?,
				source,
				prepared: None,
			})
		})
		.collect();
	(!providers.is_empty()).then_some(providers)
}

impl Provider {
	fn prepare(&mut self) -> &Prepared {
		let source = &self.source;
		self.prepared.get_or_insert_with(|| {
			let names: Vec<&str> = source
				.rules
				.iter()
				.chain(&source.referral_marketing)
				.map(String::as_str)
				.collect();
			Prepared {
				exceptions: build_all(&source.exceptions),
				params: (!names.is_empty())
					.then(|| build(&format!("^(?:{})$", names.join("|"))))
					.flatten(),
				raw_rules: build_all(&source.raw_rules),
				redirections: build_all(&source.redirections),
			}
		})
	}

	fn applies_to(&mut self, url: &str) -> bool {
		self.pattern.is_match(url)
			&& !self
				.prepare()
				.exceptions
				.iter()
				.any(|exception| exception.is_match(url))
	}
}

fn percent_decode(text: &str) -> String {
	let bytes = text.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		let decoded = (bytes[index] == b'%')
			.then(|| bytes.get(index + 1..index + 3))
			.flatten()
			.and_then(|pair| std::str::from_utf8(pair).ok())
			.and_then(|pair| u8::from_str_radix(pair, 16).ok());
		match decoded {
			Some(byte) => {
				out.push(byte);
				index += 3;
			}
			None => {
				out.push(bytes[index]);
				index += 1;
			}
		}
	}
	String::from_utf8_lossy(&out).into_owned()
}

fn strip_params(url: &str, params: &Regex) -> String {
	let (head, fragment) = match url.split_once('#') {
		Some((head, fragment)) => (head, Some(fragment)),
		None => (url, None),
	};
	let Some((base, query)) = head.split_once('?') else {
		return url.to_owned();
	};
	let kept: Vec<&str> = query
		.split('&')
		.filter(|pair| !params.is_match(pair.split('=').next().unwrap_or_default()))
		.collect();
	let mut cleaned = base.to_owned();
	if !kept.is_empty() {
		cleaned.push('?');
		cleaned.push_str(&kept.join("&"));
	}
	if let Some(fragment) = fragment {
		cleaned.push('#');
		cleaned.push_str(fragment);
	}
	cleaned
}

impl ClearUrls {
	fn use_rules(&mut self, bytes: &[u8]) -> bool {
		match parse_providers(bytes) {
			Some(providers) => {
				self.providers = providers;
				true
			}
			None => false,
		}
	}

	fn is_stale() -> bool {
		storage::get(FETCHED_KEY)
			.ok()
			.flatten()
			.and_then(|bytes| String::from_utf8(bytes).ok()?.parse::<i64>().ok())
			.is_none_or(|fetched| now_ms() - fetched > MAX_AGE_MS)
	}

	fn refresh(&mut self) {
		let request = FetchRequest {
			method: "GET".to_owned(),
			url: RULES_URL.to_owned(),
			headers: vec![("accept".to_owned(), "application/json".to_owned())],
			body: None,
		};
		let body = match host::fetch(&request) {
			Ok(response) if response.status == 200 => response.body,
			Ok(response) => {
				log::warn!("the rules download answered {}", response.status);
				return;
			}
			Err(error) => {
				log::warn!("could not download the rules: {error:?}");
				return;
			}
		};
		if !self.use_rules(&body) {
			log::warn!("the downloaded rules were not usable");
			return;
		}
		if let Err(error) = storage::set(RULES_KEY, &body)
			.and_then(|()| storage::set(FETCHED_KEY, now_ms().to_string().as_bytes()))
		{
			log::warn!("could not cache the rules: {error:?}");
		}
	}

	fn redirect_target(&mut self, url: &str) -> Option<String> {
		for provider in &mut self.providers {
			if !provider.applies_to(url) {
				continue;
			}
			let target = provider.prepare().redirections.iter().find_map(|redirection| {
				redirection
					.captures(url)?
					.get(1)
					.map(|group| percent_decode(group.as_str()))
			});
			if let Some(target) = target.filter(|target| target.starts_with("http")) {
				return Some(target);
			}
		}
		None
	}

	fn clean_url(&mut self, url: &str) -> String {
		let mut current = url.to_owned();
		for _ in 0..MAX_REDIRECTS {
			match self.redirect_target(&current) {
				Some(target) => current = target,
				None => break,
			}
		}
		for provider in &mut self.providers {
			if !provider.applies_to(&current) {
				continue;
			}
			let prepared = provider.prepare();
			if let Some(params) = &prepared.params {
				current = strip_params(&current, params);
			}
			for raw_rule in &prepared.raw_rules {
				current = raw_rule.replace_all(&current, "").into_owned();
			}
		}
		current
	}

	fn clean_content(&mut self, content: &str) -> String {
		let mut out = String::with_capacity(content.len());
		let mut rest = content;
		while let Some(start) = next_url(rest) {
			out.push_str(&rest[..start]);
			let tail = &rest[start..];
			let run = tail
				.find(|c: char| c.is_whitespace() || c == '<')
				.unwrap_or(tail.len());
			let url = tail[..run].trim_end_matches(|c| TRAILING_PUNCTUATION.contains(c));
			out.push_str(&self.clean_url(url));
			rest = &tail[url.len()..];
		}
		out.push_str(rest);
		out
	}
}

fn next_url(text: &str) -> Option<usize> {
	match (text.find("http://"), text.find("https://")) {
		(Some(plain), Some(secure)) => Some(plain.min(secure)),
		(plain, secure) => plain.or(secure),
	}
}

impl Plugin for ClearUrls {
	fn init(&mut self) -> Result<(), String> {
		let cached = storage::get(RULES_KEY).ok().flatten();
		if !cached.is_some_and(|bytes| self.use_rules(&bytes)) {
			self.use_rules(FALLBACK_RULES.as_bytes());
		}
		if let Err(error) = timer::set(FIRST_REFRESH_MS, CHECK_EVERY_MS, REFRESH_TAG) {
			log::warn!("could not schedule the rules refresh: {error:?}");
		}
		Ok(())
	}

	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		if !msg.content.contains("http") {
			return SendDecision::Pass;
		}
		let content = self.clean_content(&msg.content);
		if content == msg.content || content.is_empty() || content.len() > MAX_CONTENT_BYTES {
			SendDecision::Pass
		} else {
			SendDecision::Replace(OutgoingMessage { content, ..msg })
		}
	}
}

impl PluginExt for ClearUrls {
	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == REFRESH_TAG && Self::is_stale() {
			self.refresh();
		}
	}
}

export_plugin!(ClearUrls, ext);
