use plugin_sdk::model::{OutgoingFull, SendDecisionFull};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, client, data, export_plugin, storage};

const USER_LIST_KEY: &str = "userList";
const ROLE_LIST_KEY: &str = "roleList";
const PING_LISTED_KEY: &str = "shouldPingListed";
const SETTING_KEYS: [&str; 3] = [USER_LIST_KEY, ROLE_LIST_KEY, PING_LISTED_KEY];

struct NoReplyMention {
	users: Vec<u64>,
	roles: Vec<u64>,
	ping_listed: bool,
}

impl Default for NoReplyMention {
	fn default() -> Self {
		Self {
			users: Vec::new(),
			roles: Vec::new(),
			ping_listed: true,
		}
	}
}

fn parse_ids(raw: &str) -> Vec<u64> {
	raw.split(|character: char| !character.is_ascii_digit())
		.filter_map(|part| part.parse().ok())
		.collect()
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl NoReplyMention {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			USER_LIST_KEY => self.users = parse_ids(value),
			ROLE_LIST_KEY => self.roles = parse_ids(value),
			PING_LISTED_KEY => self.ping_listed = parse_flag(value),
			_ => {}
		}
	}

	fn is_listed(&self, author: u64, guild_id: Option<u64>) -> bool {
		if self.users.contains(&author) {
			return true;
		}
		let Some(guild_id) = guild_id.filter(|_| !self.roles.is_empty()) else {
			return false;
		};
		data::member_info(guild_id, author)
			.ok()
			.flatten()
			.is_some_and(|member| member.roles.iter().any(|role| self.roles.contains(role)))
	}
}

impl Plugin for NoReplyMention {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"userList":{"type":"string","title":"User list","description":"User ids the rule is about, separated by commas or spaces","default":""},"roleList":{"type":"string","title":"Role list","description":"Role ids the rule is about, separated by commas or spaces","default":""},"shouldPingListed":{"type":"boolean","title":"Only ping the listed users / roles","description":"On: only listed users and roles are pinged. Off: everyone except them is pinged","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for NoReplyMention {
	fn before_send_full(&mut self, msg: OutgoingFull) -> SendDecisionFull {
		let Some(reply_to) = msg.reply_to else {
			return SendDecisionFull::Pass;
		};
		let target = client::get_message(msg.channel_id, reply_to)
			.ok()
			.flatten()
			.or_else(|| data::fetch_message(msg.channel_id, reply_to).ok().flatten());
		let Some(target) = target else {
			return SendDecisionFull::Pass;
		};
		let listed = self.is_listed(target.view.author.id, target.view.guild_id);
		let mention_reply = if self.ping_listed { listed } else { !listed };
		if mention_reply == msg.mention_reply {
			SendDecisionFull::Pass
		} else {
			SendDecisionFull::Replace(OutgoingFull { mention_reply, ..msg })
		}
	}
}

export_plugin!(NoReplyMention, ext);
