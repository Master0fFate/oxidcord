use std::collections::HashMap;

use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, MessageView, Plugin, export_plugin, host, log, storage};

const SEEN_PREFIX: &str = "seen.";
const HISTORY_PREFIX: &str = "hist.";
const SHOW_KEY: &str = "show_versions";
const COUNT_KEY: &str = "version_count";
const SEEN_CAPACITY: usize = 500;
const CHUNK_SIZE: usize = 25;
const HISTORY_BUCKETS: u64 = 16;
const MAX_VERSIONS: usize = 10;
const DEFAULT_SHOWN: usize = 3;
const CONTENT_CHARS: usize = 2000;
const KEPT_VERSION_CHARS: usize = 600;
const SHOWN_VERSION_CHARS: usize = 200;

struct Entry {
	id: u64,
	content: String,
}

struct Chunk {
	seq: u64,
	entries: Vec<Entry>,
}

struct Version {
	edited_at_ms: i64,
	text: String,
}

struct EditHistory {
	chunks: Vec<Chunk>,
	history: HashMap<u64, Vec<Version>>,
	next_seq: u64,
	show_versions: bool,
	shown: usize,
}

impl Default for EditHistory {
	fn default() -> Self {
		Self {
			chunks: Vec::new(),
			history: HashMap::new(),
			next_seq: 0,
			show_versions: true,
			shown: DEFAULT_SHOWN,
		}
	}
}

impl EditHistory {
	fn locate(&self, id: u64) -> Option<(usize, usize)> {
		self.chunks.iter().enumerate().find_map(|(chunk, chunk_entries)| {
			chunk_entries
				.entries
				.iter()
				.position(|entry| entry.id == id)
				.map(|entry| (chunk, entry))
		})
	}

	fn remember(&mut self, msg: &MessageView) {
		let content = clip(&msg.content, CONTENT_CHARS).to_owned();
		if let Some((chunk, entry)) = self.locate(msg.id) {
			self.chunks[chunk].entries[entry].content = content;
			self.save_chunk(chunk);
			return;
		}
		if self
			.chunks
			.last()
			.is_none_or(|last| last.entries.len() >= CHUNK_SIZE)
		{
			self.chunks.push(Chunk {
				seq: self.next_seq,
				entries: Vec::new(),
			});
			self.next_seq += 1;
		}
		let last = self.chunks.len() - 1;
		self.chunks[last].entries.push(Entry { id: msg.id, content });
		self.save_chunk(last);
		self.evict();
	}

	fn evict(&mut self) {
		let mut total: usize = self.chunks.iter().map(|chunk| chunk.entries.len()).sum();
		let mut buckets = Vec::new();
		let mut first_dirty = false;
		while total > SEEN_CAPACITY {
			let first = &mut self.chunks[0];
			let dropped = first.entries.remove(0);
			total -= 1;
			if self.history.remove(&dropped.id).is_some() {
				buckets.push(bucket_of(dropped.id));
			}
			if first.entries.is_empty() {
				let emptied = self.chunks.remove(0);
				delete(&chunk_key(emptied.seq));
				first_dirty = false;
			} else {
				first_dirty = true;
			}
		}
		if first_dirty {
			self.save_chunk(0);
		}
		buckets.sort_unstable();
		buckets.dedup();
		for bucket in buckets {
			self.save_bucket(bucket);
		}
	}

	fn record_edit(&mut self, msg: &MessageView) {
		let Some((chunk, entry)) = self.locate(msg.id) else {
			self.remember(msg);
			return;
		};
		let content = clip(&msg.content, CONTENT_CHARS);
		let slot = &mut self.chunks[chunk].entries[entry];
		if slot.content == content {
			return;
		}
		let previous = std::mem::replace(&mut slot.content, content.to_owned());
		let versions = self.history.entry(msg.id).or_default();
		versions.push(Version {
			edited_at_ms: host::now_ms(),
			text: clip(&previous, KEPT_VERSION_CHARS).to_owned(),
		});
		if versions.len() > MAX_VERSIONS {
			versions.remove(0);
		}
		self.save_chunk(chunk);
		self.save_bucket(bucket_of(msg.id));
	}

	fn save_chunk(&self, chunk: usize) {
		let chunk = &self.chunks[chunk];
		let mut bytes = Vec::new();
		for entry in &chunk.entries {
			put_u64(&mut bytes, entry.id);
			put_str(&mut bytes, &entry.content);
		}
		set(&chunk_key(chunk.seq), &bytes);
	}

	fn save_bucket(&self, bucket: u64) {
		let mut bytes = Vec::new();
		for (id, versions) in self.history.iter().filter(|(id, _)| bucket_of(**id) == bucket) {
			put_u64(&mut bytes, *id);
			put_u64(&mut bytes, versions.len() as u64);
			for version in versions {
				put_u64(&mut bytes, version.edited_at_ms as u64);
				put_str(&mut bytes, &version.text);
			}
		}
		set(&bucket_key(bucket), &bytes);
	}

	fn load(&mut self) {
		let mut seqs: Vec<u64> = storage::keys()
			.unwrap_or_default()
			.iter()
			.filter_map(|key| key.strip_prefix(SEEN_PREFIX)?.parse().ok())
			.collect();
		seqs.sort_unstable();
		for seq in seqs {
			let Ok(Some(bytes)) = storage::get(&chunk_key(seq)) else {
				continue;
			};
			let mut reader = Reader::new(&bytes);
			let mut entries = Vec::new();
			while let (Some(id), Some(content)) = (reader.u64(), reader.string()) {
				entries.push(Entry { id, content });
			}
			if !entries.is_empty() {
				self.chunks.push(Chunk { seq, entries });
			}
			self.next_seq = seq + 1;
		}
		for bucket in 0..HISTORY_BUCKETS {
			let Ok(Some(bytes)) = storage::get(&bucket_key(bucket)) else {
				continue;
			};
			let mut reader = Reader::new(&bytes);
			while let (Some(id), Some(count)) = (reader.u64(), reader.u64()) {
				let versions: Vec<Version> = (0..count)
					.map_while(|_| {
						Some(Version {
							edited_at_ms: reader.u64()? as i64,
							text: reader.string()?,
						})
					})
					.collect();
				self.history.insert(id, versions);
			}
		}
		if let Ok(Some(value)) = storage::get(SHOW_KEY) {
			self.show_versions = value != b"false";
		}
		if let Ok(Some(value)) = storage::get(COUNT_KEY) {
			self.shown = parse_shown(&String::from_utf8_lossy(&value));
		}
	}

	fn change_setting(&mut self, key: &str, value: &str) {
		match key {
			SHOW_KEY => self.show_versions = value == "true",
			COUNT_KEY => self.shown = parse_shown(value),
			_ => return,
		}
		set(key, value.as_bytes());
	}
}

impl Plugin for EditHistory {
	fn init(&mut self) -> Result<(), String> {
		self.load();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::MessageCreated(msg) => self.remember(&msg),
			Event::MessageUpdated(msg) => self.record_edit(&msg),
			Event::SettingChanged(SettingChange { key, value }) => self.change_setting(&key, &value),
			_ => {}
		}
	}

	fn render_message(&mut self, msg: &MessageView, mut ast: Vec<Md>) -> Option<Vec<Md>> {
		let versions = self.history.get(&msg.id)?;
		let count = versions.len();
		let plural = if count == 1 { "" } else { "s" };
		ast.push(Md::subtext(format!("edited {count} time{plural}")));
		if self.show_versions {
			for version in versions.iter().rev().take(self.shown) {
				let shown = clip(&version.text, SHOWN_VERSION_CHARS);
				let ellipsis = if shown.len() < version.text.len() {
					"…"
				} else {
					""
				};
				ast.push(Md::Quote(vec![Md::text(format!("{shown}{ellipsis}"))]));
			}
		}
		Some(ast)
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"show_versions":{"type":"boolean","title":"Show previous versions","default":true},"version_count":{"type":"integer","title":"Versions to show","minimum":1,"maximum":10,"default":3}}}"#
			.to_owned()
	}
}

struct Reader<'a> {
	bytes: &'a [u8],
}

impl<'a> Reader<'a> {
	fn new(bytes: &'a [u8]) -> Self {
		Self { bytes }
	}

	fn take(&mut self, len: usize) -> Option<&'a [u8]> {
		let (head, tail) = self.bytes.split_at_checked(len)?;
		self.bytes = tail;
		Some(head)
	}

	fn u64(&mut self) -> Option<u64> {
		Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
	}

	fn string(&mut self) -> Option<String> {
		let len = usize::try_from(self.u64()?).ok()?;
		Some(String::from_utf8_lossy(self.take(len)?).into_owned())
	}
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
	bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_str(bytes: &mut Vec<u8>, value: &str) {
	put_u64(bytes, value.len() as u64);
	bytes.extend_from_slice(value.as_bytes());
}

fn clip(text: &str, max_chars: usize) -> &str {
	text.char_indices()
		.nth(max_chars)
		.map_or(text, |(end, _)| &text[..end])
}

fn parse_shown(value: &str) -> usize {
	value
		.trim()
		.parse()
		.map_or(DEFAULT_SHOWN, |count: usize| count.clamp(1, MAX_VERSIONS))
}

fn bucket_of(id: u64) -> u64 {
	id % HISTORY_BUCKETS
}

fn chunk_key(seq: u64) -> String {
	format!("{SEEN_PREFIX}{seq}")
}

fn bucket_key(bucket: u64) -> String {
	format!("{HISTORY_PREFIX}{bucket}")
}

fn set(key: &str, value: &[u8]) {
	if let Err(error) = storage::set(key, value) {
		log::warn!("could not write {key}: {error:?}");
	}
}

fn delete(key: &str) {
	if let Err(error) = storage::delete(key) {
		log::warn!("could not delete {key}: {error:?}");
	}
}

export_plugin!(EditHistory);
