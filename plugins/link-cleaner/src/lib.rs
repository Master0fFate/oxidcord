use plugin_sdk::{OutgoingMessage, Plugin, SendDecision, export_plugin};

const TRACKING_KEYS: [&str; 4] = ["fbclid", "gclid", "igshid", "ref_src"];
const SI_HOSTS: [&str; 4] = ["youtube.com", "youtu.be", "spotify.com", "spotify.link"];
const S_HOSTS: [&str; 2] = ["twitter.com", "x.com"];

#[derive(Default)]
struct LinkCleaner;

impl Plugin for LinkCleaner {
	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		let content = clean_content(&msg.content);
		if content == msg.content {
			SendDecision::Pass
		} else {
			SendDecision::Replace(OutgoingMessage { content, ..msg })
		}
	}
}

fn clean_content(content: &str) -> String {
	let mut cleaned = String::with_capacity(content.len());
	let mut rest = content;
	while let Some(start) = find_url_start(rest) {
		cleaned.push_str(&rest[..start]);
		let tail = &rest[start..];
		let end = tail.find(is_url_end).unwrap_or(tail.len());
		cleaned.push_str(&clean_url(&tail[..end]));
		rest = &tail[end..];
	}
	cleaned.push_str(rest);
	cleaned
}

fn find_url_start(text: &str) -> Option<usize> {
	match (text.find("http://"), text.find("https://")) {
		(Some(plain), Some(secure)) => Some(plain.min(secure)),
		(plain, secure) => plain.or(secure),
	}
}

fn is_url_end(character: char) -> bool {
	character.is_whitespace() || matches!(character, '<' | '>' | '"' | ')' | ']')
}

fn clean_url(url: &str) -> String {
	let (without_fragment, fragment) = match url.split_once('#') {
		Some((head, fragment)) => (head, Some(fragment)),
		None => (url, None),
	};
	let Some((base, query)) = without_fragment.split_once('?') else {
		return url.to_owned();
	};
	let host = host_of(base);
	let kept: Vec<&str> = query
		.split('&')
		.filter(|pair| !is_tracking(pair.split('=').next().unwrap_or_default(), &host))
		.collect();
	if kept.len() == query.split('&').count() {
		return url.to_owned();
	}
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

fn host_of(base: &str) -> String {
	let after_scheme = base.split_once("://").map_or(base, |(_, rest)| rest);
	let authority = after_scheme.split('/').next().unwrap_or_default();
	authority
		.rsplit('@')
		.next()
		.unwrap_or_default()
		.split(':')
		.next()
		.unwrap_or_default()
		.to_ascii_lowercase()
}

fn is_tracking(key: &str, host: &str) -> bool {
	let key = key.to_ascii_lowercase();
	key.starts_with("utm_")
		|| TRACKING_KEYS.contains(&key.as_str())
		|| (key == "si" && matches_host(host, &SI_HOSTS))
		|| (key == "s" && matches_host(host, &S_HOSTS))
}

fn matches_host(host: &str, domains: &[&str]) -> bool {
	domains
		.iter()
		.any(|domain| host == *domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.')))
}

export_plugin!(LinkCleaner);
