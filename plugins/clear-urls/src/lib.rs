use plugin_sdk::{OutgoingMessage, Plugin, SendDecision, export_plugin};
use regex_lite::{Regex, RegexBuilder};

const TRACKING_PARAMS: &str = r"^(?:utm|utm_.*|ga_.*|fbclid|gclid|dclid|msclkid|twclid|ttclid|yclid|gbraid|wbraid|mc_eid|mc_cid|mc_tc|mkt_tok|igshid|_ga|_gl|srsltid|ref_src|fb_source|fb_ref)$";
const MAX_CONTENT_BYTES: usize = 4000;
const TRAILING_PUNCTUATION: &str = ".,:;\"'>)|]";

#[derive(Default)]
struct ClearUrls {
	params: Option<Regex>,
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

fn is_youtube(base: &str) -> bool {
	let Some((_, rest)) = base.split_once("://") else {
		return false;
	};
	let authority = rest.split('/').next().unwrap_or_default();
	let host_port = authority.rsplit('@').next().unwrap_or_default();
	let host = match host_port.split_once(':') {
		Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => host,
		Some(_) => return false,
		None => host_port,
	};
	let mut suffix = host;
	loop {
		if suffix.eq_ignore_ascii_case("youtube.com") || suffix.eq_ignore_ascii_case("youtu.be") {
			return true;
		}
		match suffix.split_once('.') {
			Some((_, rest)) => suffix = rest,
			None => return false,
		}
	}
}

fn strip_params(url: &str, params: &Regex) -> String {
	let (head, fragment) = match url.split_once('#') {
		Some((head, fragment)) => (head, Some(fragment)),
		None => (url, None),
	};
	let Some((base, query)) = head.split_once('?') else {
		return url.to_owned();
	};
	let youtube = is_youtube(base);
	let kept: Vec<&str> = query
		.split('&')
		.filter(|pair| {
			let key = pair.split('=').next().unwrap_or_default();
			let decoded;
			let key = if key.contains('%') {
				decoded = percent_decode(key);
				decoded.as_str()
			} else {
				key
			};
			!params.is_match(key) && !(youtube && key.eq_ignore_ascii_case("si"))
		})
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
	fn clean_content(&self, content: &str, params: &Regex) -> String {
		let mut out = String::with_capacity(content.len());
		let mut rest = content;
		while let Some(start) = next_url(rest) {
			out.push_str(&rest[..start]);
			let tail = &rest[start..];
			let run = tail
				.find(|c: char| c.is_whitespace() || c == '<')
				.unwrap_or(tail.len());
			let url = tail[..run].trim_end_matches(|c| TRAILING_PUNCTUATION.contains(c));
			out.push_str(&strip_params(url, params));
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
		self.params = Some(
			RegexBuilder::new(TRACKING_PARAMS)
				.case_insensitive(true)
				.build()
				.map_err(|error| error.to_string())?,
		);
		Ok(())
	}

	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		let Some(params) = &self.params else {
			return SendDecision::Pass;
		};
		if !msg.content.contains("http") {
			return SendDecision::Pass;
		}
		let content = self.clean_content(&msg.content, params);
		if content == msg.content || content.is_empty() || content.len() > MAX_CONTENT_BYTES {
			SendDecision::Pass
		} else {
			SendDecision::Replace(OutgoingMessage { content, ..msg })
		}
	}
}

export_plugin!(ClearUrls);
