use std::collections::VecDeque;

use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, MessageView, Plugin, export_plugin, host, log, storage};

const KEYWORDS_KEY: &str = "keywords";
const NOTIFIED_CAPACITY: usize = 256;
const FRESH_WINDOW_MS: i64 = 60_000;
const BODY_CHARS: usize = 140;

#[derive(Default)]
struct KeywordHighlighter {
	keywords: Vec<String>,
	notified: VecDeque<u64>,
}

impl KeywordHighlighter {
	fn set_keywords(&mut self, raw: &str) {
		self.keywords = parse_keywords(raw);
		if let Err(error) = storage::set(KEYWORDS_KEY, self.keywords.join("\n").as_bytes()) {
			log::warn!("could not save keywords: {error:?}");
		}
	}

	fn notify_once(&mut self, msg: &MessageView, hits: &[String]) {
		let fresh = host::now_ms() - msg.timestamp_ms < FRESH_WINDOW_MS;
		if !fresh || self.notified.contains(&msg.id) {
			return;
		}
		if self.notified.len() == NOTIFIED_CAPACITY {
			self.notified.pop_front();
		}
		self.notified.push_back(msg.id);
		match host::current_user() {
			Ok(me) if me.id != msg.author.id => {
				let title = format!("{} mentioned {}", msg.author.display_name, hits.join(", "));
				let body: String = msg.content.chars().take(BODY_CHARS).collect();
				if let Err(error) = host::notify(&title, &body) {
					log::debug!("notification skipped: {error:?}");
				}
			}
			_ => {}
		}
	}
}

impl Plugin for KeywordHighlighter {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(KEYWORDS_KEY) {
			self.keywords = parse_keywords(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == KEYWORDS_KEY
		{
			self.set_keywords(&value);
		}
	}

	fn render_message(&mut self, msg: &MessageView, ast: Vec<Md>) -> Option<Vec<Md>> {
		if self.keywords.is_empty() {
			return None;
		}
		let mut hits = Vec::new();
		let mut nodes = highlight(ast, &self.keywords, &mut hits);
		if hits.is_empty() {
			return None;
		}
		nodes.push(Md::subtext(format!("keyword: {}", hits.join(", "))));
		self.notify_once(msg, &hits);
		Some(nodes)
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"keywords":{"type":"array","title":"Keywords","items":{"type":"string"},"default":[]}}}"#
			.to_owned()
	}
}

fn parse_keywords(raw: &str) -> Vec<String> {
	raw.split([',', '\n'])
		.map(|word| word.trim_matches(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '"')))
		.filter(|word| !word.is_empty())
		.map(str::to_ascii_lowercase)
		.collect()
}

fn highlight(nodes: Vec<Md>, keywords: &[String], hits: &mut Vec<String>) -> Vec<Md> {
	nodes
		.into_iter()
		.flat_map(|node| match node {
			Md::Text(text) => split_matches(&text, keywords, hits),
			Md::Italic(children) => vec![Md::Italic(highlight(children, keywords, hits))],
			Md::Underline(children) => vec![Md::Underline(highlight(children, keywords, hits))],
			Md::Strike(children) => vec![Md::Strike(highlight(children, keywords, hits))],
			Md::Quote(children) => vec![Md::Quote(highlight(children, keywords, hits))],
			Md::Spoiler(children) => vec![Md::Spoiler(highlight(children, keywords, hits))],
			other => vec![other],
		})
		.collect()
}

fn split_matches(text: &str, keywords: &[String], hits: &mut Vec<String>) -> Vec<Md> {
	let lowered = text.to_ascii_lowercase();
	let mut parts = Vec::new();
	let mut from = 0;
	while let Some((start, keyword)) = keywords
		.iter()
		.filter_map(|keyword| {
			lowered[from..]
				.find(keyword.as_str())
				.map(|at| (from + at, keyword))
		})
		.min_by_key(|(start, keyword)| (*start, std::cmp::Reverse(keyword.len())))
	{
		let end = start + keyword.len();
		if start > from {
			parts.push(Md::text(&text[from..start]));
		}
		parts.push(Md::Bold(vec![Md::text(&text[start..end])]));
		if !hits.contains(keyword) {
			hits.push(keyword.clone());
		}
		from = end;
	}
	if from < text.len() {
		parts.push(Md::text(&text[from..]));
	}
	parts
}

export_plugin!(KeywordHighlighter);
