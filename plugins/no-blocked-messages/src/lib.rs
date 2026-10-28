use std::collections::HashSet;

use plugin_sdk::model::{ClientEvent, RelationshipKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, MessageView, Plugin, PluginExt, data, export_plugin, log, storage};

const APPLY_KEY: &str = "applyToIgnoredUsers";
const IGNORED_KEY: &str = "ignoredUsers";

struct NoBlockedMessages {
	blocked: HashSet<u64>,
	ignored: HashSet<u64>,
	apply_to_ignored: bool,
}

impl Default for NoBlockedMessages {
	fn default() -> Self {
		Self {
			blocked: HashSet::new(),
			ignored: HashSet::new(),
			apply_to_ignored: true,
		}
	}
}

fn parse_ids(raw: &str) -> HashSet<u64> {
	raw.split(|c: char| !c.is_ascii_digit())
		.filter_map(|part| part.parse().ok())
		.collect()
}

impl NoBlockedMessages {
	fn load_blocked(&mut self) {
		match data::list_relationships() {
			Ok(people) => {
				self.blocked = people
					.into_iter()
					.filter(|relationship| relationship.kind == RelationshipKind::Blocked)
					.map(|relationship| relationship.user.id)
					.collect();
			}
			Err(error) => log::warn!("could not read blocked users: {error:?}"),
		}
	}
}

impl Plugin for NoBlockedMessages {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{APPLY_KEY}")) {
			self.apply_to_ignored = !matches!(String::from_utf8_lossy(&saved).trim(), "false" | "0" | "off");
		}
		if let Ok(Some(saved)) = storage::get(&format!("settings.{IGNORED_KEY}")) {
			self.ignored = parse_ids(&String::from_utf8_lossy(&saved));
		}
		self.load_blocked();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		let Event::SettingChanged(SettingChange { key, value }) = event else {
			return;
		};
		match key.as_str() {
			APPLY_KEY => self.apply_to_ignored = !matches!(value.trim(), "false" | "0" | "off"),
			IGNORED_KEY => self.ignored = parse_ids(&value),
			_ => {}
		}
	}

	fn filter_message(&mut self, msg: &MessageView) -> bool {
		let author = msg.author.id;
		!(self.blocked.contains(&author) || (self.apply_to_ignored && self.ignored.contains(&author)))
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"applyToIgnoredUsers":{"type":"boolean","title":"Also hide ignored users","description":"Additionally hide messages from the users listed in ignoredUsers","default":true},"ignoredUsers":{"type":"array","title":"Ignored users","description":"User ids whose messages are hidden like blocked ones","items":{"type":"string"},"default":[]}}}"#
			.to_owned()
	}
}

impl PluginExt for NoBlockedMessages {
	fn on_client_event(&mut self, event: ClientEvent) {
		match event {
			ClientEvent::RelationshipChanged(relationship) => {
				if relationship.kind == RelationshipKind::Blocked {
					self.blocked.insert(relationship.user.id);
				} else {
					self.blocked.remove(&relationship.user.id);
				}
			}
			ClientEvent::RelationshipRemoved(id) => {
				self.blocked.remove(&id);
			}
			ClientEvent::Ready => self.load_blocked(),
			_ => {}
		}
	}
}

export_plugin!(NoBlockedMessages, ext);
