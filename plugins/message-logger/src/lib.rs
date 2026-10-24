use std::collections::{HashMap, HashSet, VecDeque};

use plugin_sdk::ui::{self, button, column, text};
use plugin_sdk::{Event, Md, MessageView, Plugin, Ui, export_plugin, is_deleted, log, storage};

const ACCESSORY_SLOT: &str = "message-accessory";
const TARGET_PREFIX: &str = "msg-";
const TOGGLE_PREFIX: &str = "history:";
const HISTORY_PREFIX: &str = "edits.";
const SEEN_CAPACITY: usize = 1000;
const LOGGED_CAPACITY: usize = 200;
const MAX_VERSIONS: usize = 10;
const KEPT_CHARS: usize = 2000;
const SHOWN_CHARS: usize = 300;

#[derive(Default)]
struct MessageLogger {
	seen: HashMap<u64, String>,
	seen_order: VecDeque<u64>,
	history: HashMap<u64, Vec<String>>,
	logged: VecDeque<u64>,
	expanded: HashSet<u64>,
}

impl MessageLogger {
	fn observe(&mut self, msg: &MessageView) -> bool {
		let content = clip(&msg.content, KEPT_CHARS);
		match self.seen.get_mut(&msg.id) {
			Some(previous) if previous != content => {
				let earlier = std::mem::replace(previous, content.to_owned());
				self.record(msg.id, earlier);
				true
			}
			Some(_) => false,
			None => {
				self.seen.insert(msg.id, content.to_owned());
				self.seen_order.push_back(msg.id);
				if self.seen_order.len() > SEEN_CAPACITY
					&& let Some(oldest) = self.seen_order.pop_front()
				{
					self.seen.remove(&oldest);
				}
				false
			}
		}
	}

	fn record(&mut self, id: u64, earlier: String) {
		let versions = self.history.entry(id).or_default();
		versions.push(earlier);
		if versions.len() > MAX_VERSIONS {
			versions.remove(0);
		}
		save(id, versions);
		self.logged.retain(|logged| *logged != id);
		self.logged.push_back(id);
		if self.logged.len() > LOGGED_CAPACITY
			&& let Some(dropped) = self.logged.pop_front()
		{
			self.history.remove(&dropped);
			self.expanded.remove(&dropped);
			if let Err(error) = storage::delete(&history_key(dropped)) {
				log::warn!("could not forget the history of {dropped}: {error:?}");
			}
		}
	}

	fn load(&mut self) {
		let mut ids: Vec<u64> = storage::keys()
			.unwrap_or_default()
			.iter()
			.filter_map(|key| key.strip_prefix(HISTORY_PREFIX)?.parse().ok())
			.collect();
		ids.sort_unstable();
		for id in ids {
			let Ok(Some(bytes)) = storage::get(&history_key(id)) else {
				continue;
			};
			let versions = decode(&bytes);
			if !versions.is_empty() {
				self.history.insert(id, versions);
				self.logged.push_back(id);
			}
		}
	}

	fn tree(&self) -> Vec<Ui> {
		let mut root = column();
		for id in &self.logged {
			let Some(versions) = self.history.get(id) else {
				continue;
			};
			let toggle = format!("{TOGGLE_PREFIX}{id}");
			let mut entry = column().id(format!("{TARGET_PREFIX}{id}")).gap(4);
			if self.expanded.contains(id) {
				for (index, version) in versions.iter().enumerate().rev() {
					let label = if index == 0 {
						"Original".to_owned()
					} else {
						format!("Edit {index}")
					};
					entry = entry
						.child(text(label).muted().size(11))
						.child(text(clip(version, SHOWN_CHARS)).muted());
				}
				entry = entry.child(button(toggle, "Hide history").variant("ghost").icon("clock"));
			} else {
				let count = versions.len();
				let plural = if count == 1 { "" } else { "s" };
				entry = entry.child(
					button(toggle, format!("{count} earlier version{plural}"))
						.variant("ghost")
						.icon("clock"),
				);
			}
			root = root.child(entry);
		}
		vec![root.into()]
	}

	fn publish(&self) {
		if let Err(error) = ui::register_slot(ACCESSORY_SLOT, &self.tree()) {
			log::warn!("could not register the {ACCESSORY_SLOT} slot: {error:?}");
		}
	}

	fn see(&mut self, msg: &MessageView) {
		if self.observe(msg) {
			self.publish();
		}
	}
}

impl Plugin for MessageLogger {
	fn init(&mut self) -> Result<(), String> {
		self.load();
		if !self.logged.is_empty() {
			self.publish();
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::MessageCreated(msg) | Event::MessageUpdated(msg) = event {
			self.see(&msg);
		}
	}

	fn render_message(&mut self, msg: &MessageView, ast: Vec<Md>) -> Option<Vec<Md>> {
		if is_deleted(msg) {
			return Some(ast);
		}
		self.see(msg);
		None
	}

	fn on_ui_action(&mut self, id: &str, _payload: &str) -> Vec<Ui> {
		let Some(target) = id.strip_prefix(TOGGLE_PREFIX).and_then(|id| id.parse().ok()) else {
			return Vec::new();
		};
		if !self.expanded.remove(&target) {
			self.expanded.insert(target);
		}
		self.tree()
	}
}

fn history_key(id: u64) -> String {
	format!("{HISTORY_PREFIX}{id}")
}

fn save(id: u64, versions: &[String]) {
	let mut bytes = Vec::new();
	for version in versions {
		bytes.extend_from_slice(&(version.len() as u64).to_le_bytes());
		bytes.extend_from_slice(version.as_bytes());
	}
	if let Err(error) = storage::set(&history_key(id), &bytes) {
		log::warn!("could not save the history of {id}: {error:?}");
	}
}

fn decode(mut bytes: &[u8]) -> Vec<String> {
	let mut versions = Vec::new();
	while let Some((length, rest)) = bytes.split_first_chunk::<8>() {
		let Some((version, rest)) = usize::try_from(u64::from_le_bytes(*length))
			.ok()
			.and_then(|length| rest.split_at_checked(length))
		else {
			break;
		};
		versions.push(String::from_utf8_lossy(version).into_owned());
		bytes = rest;
	}
	versions
}

fn clip(text: &str, max_chars: usize) -> &str {
	text.char_indices()
		.nth(max_chars)
		.map_or(text, |(end, _)| &text[..end])
}

export_plugin!(MessageLogger);
