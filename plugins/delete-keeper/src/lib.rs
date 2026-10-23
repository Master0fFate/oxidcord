use std::collections::VecDeque;

use plugin_sdk::ui::{self, badge, column, text};
use plugin_sdk::{Event, MessageView, Plugin, Ui, export_plugin, log, storage};

const SEEN_INDEX_KEY: &str = "index.seen";
const DELETED_INDEX_KEY: &str = "index.deleted";
const SEEN_PREFIX: &str = "seen.";
const DELETED_PREFIX: &str = "deleted.";
const ACCESSORY_SLOT: &str = "message-accessory";
const SEEN_CAPACITY: usize = 500;
const DELETED_CAPACITY: usize = 25;
const SHOWN_CHARS: usize = 500;
const DELETED_COLOR: &str = "#f23f43";

#[derive(Default)]
struct DeleteKeeper {
	seen: VecDeque<u64>,
	deleted: VecDeque<u64>,
}

impl DeleteKeeper {
	fn remember(&mut self, msg: &MessageView) {
		self.seen.retain(|id| *id != msg.id);
		self.seen.push_back(msg.id);
		set(&format!("{SEEN_PREFIX}{}", msg.id), msg.content.as_bytes());
		while self.seen.len() > SEEN_CAPACITY {
			if let Some(evicted) = self.seen.pop_front() {
				delete(&format!("{SEEN_PREFIX}{evicted}"));
			}
		}
		save_index(SEEN_INDEX_KEY, &self.seen);
	}

	fn keep_deleted(&mut self, id: u64) {
		let seen_key = format!("{SEEN_PREFIX}{id}");
		let Ok(Some(content)) = storage::get(&seen_key) else {
			return;
		};
		self.seen.retain(|seen| *seen != id);
		delete(&seen_key);
		set(&format!("{DELETED_PREFIX}{id}"), &content);
		self.deleted.push_back(id);
		while self.deleted.len() > DELETED_CAPACITY {
			if let Some(dropped) = self.deleted.pop_front() {
				delete(&format!("{DELETED_PREFIX}{dropped}"));
			}
		}
		save_index(SEEN_INDEX_KEY, &self.seen);
		save_index(DELETED_INDEX_KEY, &self.deleted);
	}

	fn publish(&self) {
		let mut kept = column().gap(4);
		for id in &self.deleted {
			let Ok(Some(content)) = storage::get(&format!("{DELETED_PREFIX}{id}")) else {
				continue;
			};
			let content = String::from_utf8_lossy(&content);
			let shown: String = content.chars().take(SHOWN_CHARS).collect();
			kept = kept.child(
				column()
					.id(format!("deleted-{id}"))
					.gap(2)
					.child(badge("deleted").color(DELETED_COLOR))
					.child(text(shown).muted()),
			);
		}
		if let Err(error) = ui::register_slot(ACCESSORY_SLOT, &[Ui::from(kept)]) {
			log::warn!("could not register the {ACCESSORY_SLOT} slot: {error:?}");
		}
	}
}

impl Plugin for DeleteKeeper {
	fn init(&mut self) -> Result<(), String> {
		self.seen = load_index(SEEN_INDEX_KEY);
		self.deleted = load_index(DELETED_INDEX_KEY);
		if !self.deleted.is_empty() {
			self.publish();
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::MessageCreated(msg) | Event::MessageUpdated(msg) => self.remember(&msg),
			Event::MessageDeleted(message) => {
				self.keep_deleted(message.id);
				self.publish();
			}
			_ => {}
		}
	}
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

fn load_index(key: &str) -> VecDeque<u64> {
	let Ok(Some(bytes)) = storage::get(key) else {
		return VecDeque::new();
	};
	String::from_utf8_lossy(&bytes)
		.lines()
		.filter_map(|line| line.parse().ok())
		.collect()
}

fn save_index(key: &str, ids: &VecDeque<u64>) {
	let joined: Vec<String> = ids.iter().map(u64::to_string).collect();
	set(key, joined.join("\n").as_bytes());
}

export_plugin!(DeleteKeeper);
