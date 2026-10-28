use std::collections::HashSet;

use plugin_sdk::model::{ClientEvent, NotificationInfo, NotificationKind, NotificationVerdict};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, data, export_plugin, storage};

const SETTING_KEYS: [&str; 3] = ["channelToAffect", "allowMentions", "allowEveryone"];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Affected {
	#[default]
	Both,
	Dm,
	Group,
}

#[derive(Default)]
struct Config {
	affected: Affected,
	allow_mentions: bool,
	allow_everyone: bool,
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		match key {
			"channelToAffect" => {
				self.affected = match value {
					"user_dm" => Affected::Dm,
					"group_dm" => Affected::Group,
					_ => Affected::Both,
				};
			}
			"allowMentions" => self.allow_mentions = matches!(value, "true" | "1" | "on"),
			"allowEveryone" => self.allow_everyone = matches!(value, "true" | "1" | "on"),
			_ => {}
		}
	}

	fn covers(&self, kind: NotificationKind) -> bool {
		match kind {
			NotificationKind::Dm => self.affected != Affected::Group,
			NotificationKind::GroupDm => self.affected != Affected::Dm,
			NotificationKind::Message => false,
		}
	}
}

#[derive(Default)]
struct OnePingPerDm {
	config: Config,
	pinged: HashSet<u64>,
}

impl Plugin for OnePingPerDm {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"channelToAffect":{"type":"string","title":"Affected chats","description":"both_dms limits every direct message and group chat, user_dm only direct messages, group_dm only group chats","enum":["both_dms","user_dm","group_dm"],"default":"both_dms"},"allowMentions":{"type":"boolean","title":"Allow mentions","description":"Receive audio pings for @mentions","default":false},"allowEveryone":{"type":"boolean","title":"Allow everyone","description":"Receive audio pings for @everyone and @here in group chats","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for OnePingPerDm {
	fn on_notification(&mut self, info: &NotificationInfo) -> NotificationVerdict {
		if !self.config.covers(info.kind)
			|| (self.config.allow_mentions && info.mentions_me)
			|| (self.config.allow_everyone && info.mentions_everyone)
		{
			return NotificationVerdict::Allow;
		}
		if self.pinged.contains(&info.channel_id) {
			let read = data::read_state(info.channel_id)
				.ok()
				.flatten()
				.is_some_and(|state| !state.unread);
			if !read {
				return NotificationVerdict::Silent;
			}
		}
		self.pinged.insert(info.channel_id);
		NotificationVerdict::Allow
	}

	fn on_client_event(&mut self, event: ClientEvent) {
		if let ClientEvent::UnreadChanged(state) = event
			&& !state.unread
		{
			self.pinged.remove(&state.channel_id);
		}
	}
}

export_plugin!(OnePingPerDm, ext);
