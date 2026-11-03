use std::collections::{HashMap, HashSet};

use chrono::Utc;
use protocol::{
	FRIEND_SOURCES_ALL, ProtoCustomStatus, ProtoGuildFolder, ProtoGuildFolders, ProtoSettings, Snowflake,
	UserGuildSettings, decode_friend_source_flags, decode_settings, decode_status, encode_settings,
	privacy_with_friend_sources, status_with_custom, status_with_show_current_game, status_with_value,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::changes::Changes;
use crate::state::State;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildFolder {
	pub id: Option<i64>,
	pub name: Option<String>,
	pub color: Option<u32>,
	pub guild_ids: Vec<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailEntry {
	Guild(Snowflake),
	Folder {
		id: u64,
		name: Option<String>,
		color: Option<u32>,
		guilds: Vec<Snowflake>,
	},
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawFolder {
	id: Option<i64>,
	name: Option<String>,
	color: Option<i64>,
	guild_ids: Vec<Snowflake>,
}

impl From<RawFolder> for GuildFolder {
	fn from(raw: RawFolder) -> Self {
		Self {
			id: raw.id,
			name: raw.name,
			color: raw.color.and_then(|color| u32::try_from(color).ok()),
			guild_ids: raw.guild_ids,
		}
	}
}

impl State {
	pub fn custom_status(&self) -> Option<&ProtoCustomStatus> {
		self.status_settings.custom_status.as_ref()
	}

	pub fn custom_status_expiry_ms(&self) -> Option<u64> {
		self.custom_status()?.expires_at_ms
	}

	pub fn show_current_game(&self) -> bool {
		self.status_settings.show_current_game
	}

	pub fn friend_source_flags(&self) -> u32 {
		self.friend_source_flags.unwrap_or(FRIEND_SOURCES_ALL)
	}

	pub fn custom_status_patch(&mut self, custom: Option<&ProtoCustomStatus>) -> String {
		let status = status_with_custom(&self.proto_status, custom);
		self.status_patch(status)
	}

	pub fn show_current_game_patch(&mut self, shown: bool) -> String {
		let status = status_with_show_current_game(&self.proto_status, shown);
		self.status_patch(status)
	}

	pub fn friend_sources_patch(&mut self, flags: u32) -> String {
		self.proto_privacy = privacy_with_friend_sources(&self.proto_privacy, flags);
		self.friend_source_flags = decode_friend_source_flags(&self.proto_privacy);
		encode_settings(&ProtoSettings {
			privacy_bytes: Some(self.proto_privacy.clone()),
			..ProtoSettings::default()
		})
	}

	fn status_patch(&mut self, mut status: Vec<u8>) -> String {
		if let Some(chosen) = &self.chosen_status {
			status = status_with_value(&status, chosen.as_str());
		}
		self.proto_status = status;
		encode_settings(&ProtoSettings {
			status_bytes: Some(self.proto_status.clone()),
			..ProtoSettings::default()
		})
	}

	pub fn guild_layout(&self) -> Vec<RailEntry> {
		let mut placed: HashSet<Snowflake> = HashSet::new();
		let mut layout = Vec::new();
		for folder in &self.guild_folders {
			let guilds: Vec<Snowflake> = folder
				.guild_ids
				.iter()
				.copied()
				.filter(|id| self.guilds.contains_key(id) && placed.insert(*id))
				.collect();
			match folder.id {
				Some(id) if !guilds.is_empty() => layout.push(RailEntry::Folder {
					id: id as u64,
					name: folder.name.clone(),
					color: folder.color,
					guilds,
				}),
				_ => layout.extend(guilds.into_iter().map(RailEntry::Guild)),
			}
		}
		layout.extend(
			self.guilds
				.keys()
				.copied()
				.filter(|id| !placed.contains(id))
				.map(RailEntry::Guild),
		);
		layout
	}

	pub(crate) fn apply_settings_proto(&mut self, encoded: &str, partial: bool) -> Changes {
		let mut changes = Changes::default();
		let Some(ProtoSettings {
			status_bytes,
			privacy_bytes,
			guild_folders,
		}) = decode_settings(encoded)
		else {
			return changes;
		};
		if status_bytes.is_some() || !partial {
			changes.merge(self.replace_status_settings(status_bytes.unwrap_or_default()));
		}
		if privacy_bytes.is_some() || !partial {
			changes.merge(self.replace_privacy_settings(privacy_bytes.unwrap_or_default()));
		}
		if let Some(ProtoGuildFolders { folders, positions }) = guild_folders {
			self.guild_folders = folders.into_iter().map(GuildFolder::from).collect();
			self.guild_positions = positions;
			self.reorder_guilds();
			changes.guild_list = true;
		}
		changes
	}

	fn replace_status_settings(&mut self, bytes: Vec<u8>) -> Changes {
		let mut bytes = bytes;
		let mut status = decode_status(&bytes);
		let now_ms = now_ms();
		if status
			.custom_status
			.as_ref()
			.is_some_and(|custom| custom.expired_at(now_ms))
		{
			status.custom_status = None;
			bytes = status_with_custom(&bytes, None);
		}
		let mut changes = Changes::default();
		if let Some(chosen) = &status.status {
			changes.own_status = self.restore_status(chosen);
		}
		if status != self.status_settings {
			changes.own_status |= status.custom_status != self.status_settings.custom_status;
			changes.user_settings = true;
		}
		self.proto_status = bytes;
		self.status_settings = status;
		changes
	}

	fn replace_privacy_settings(&mut self, bytes: Vec<u8>) -> Changes {
		let flags = decode_friend_source_flags(&bytes);
		let changed = flags != self.friend_source_flags;
		self.proto_privacy = bytes;
		self.friend_source_flags = flags;
		Changes {
			user_settings: changed,
			..Changes::default()
		}
	}

	pub(crate) fn expire_custom_status(&mut self) -> Changes {
		let expired = self
			.custom_status()
			.is_some_and(|custom| custom.expired_at(now_ms()));
		if expired {
			self.status_settings.custom_status = None;
			self.proto_status = status_with_custom(&self.proto_status, None);
		}
		Changes {
			own_status: expired,
			user_settings: expired,
			..Changes::default()
		}
	}

	pub(crate) fn apply_settings_proto_update(&mut self, update: &Value) -> Changes {
		let settings = update.get("settings");
		let proto = settings
			.and_then(|settings| settings.get("proto"))
			.and_then(Value::as_str);
		let kind = settings
			.and_then(|settings| settings.get("type"))
			.and_then(Value::as_i64);
		let partial = update.get("partial").and_then(Value::as_bool).unwrap_or(true);
		match proto {
			Some(encoded) if kind.is_none_or(|kind| kind == 1) => self.apply_settings_proto(encoded, partial),
			_ => Changes::default(),
		}
	}

	pub(crate) fn apply_user_settings(&mut self, settings: &Value) -> Changes {
		let mut changes = Changes::default();
		if let Some(status) = settings.get("status").and_then(Value::as_str) {
			changes.own_status = self.restore_status(status);
		}
		if let Some(folders) = settings.get("guild_folders").and_then(Value::as_array) {
			self.guild_folders = folders
				.iter()
				.filter_map(|folder| RawFolder::deserialize(folder).ok())
				.map(GuildFolder::from)
				.collect();
			changes.guild_list = true;
		}
		if let Some(positions) = settings.get("guild_positions").and_then(Value::as_array) {
			self.guild_positions = positions
				.iter()
				.filter_map(|position| Snowflake::deserialize(position).ok())
				.collect();
			changes.guild_list = true;
		}
		if changes.guild_list {
			self.reorder_guilds();
		}
		changes
	}

	pub(crate) fn reorder_guilds(&mut self) {
		let order: Vec<Snowflake> = if self.guild_folders.is_empty() {
			self.guild_positions.clone()
		} else {
			self.guild_folders
				.iter()
				.flat_map(|folder| folder.guild_ids.iter().copied())
				.collect()
		};
		if order.is_empty() {
			return;
		}
		let ranks: HashMap<Snowflake, usize> = order
			.into_iter()
			.enumerate()
			.map(|(rank, id)| (id, rank))
			.collect();
		self.guilds
			.sort_by_cached_key(|id, _| ranks.get(id).copied().unwrap_or(usize::MAX));
	}

	pub(crate) fn store_guild_settings(&mut self, settings: &UserGuildSettings) {
		for over in settings.channel_overrides.iter() {
			match over.collapsed {
				Some(true) => {
					self.collapsed_categories.insert(over.channel_id);
				}
				Some(false) => {
					self.collapsed_categories.remove(&over.channel_id);
				}
				None => {}
			}
		}
		match settings.guild_id {
			Some(id) => {
				self.guild_settings.insert(id, settings.clone());
			}
			None => self.global_settings = Some(settings.clone()),
		}
	}

	pub(crate) fn guild_folder_index(&self, guild_id: Snowflake) -> Option<usize> {
		self.guild_folders
			.iter()
			.position(|folder| folder.id.is_some() && folder.guild_ids.contains(&guild_id))
	}
}

fn now_ms() -> u64 {
	u64::try_from(Utc::now().timestamp_millis()).unwrap_or_default()
}

impl From<ProtoGuildFolder> for GuildFolder {
	fn from(proto: ProtoGuildFolder) -> Self {
		Self {
			id: proto.id,
			name: proto.name,
			color: proto.color.and_then(|color| u32::try_from(color).ok()),
			guild_ids: proto.guild_ids,
		}
	}
}

pub fn guild_folders_settings(layout: &[RailEntry]) -> Value {
	let folders: Vec<Value> = layout
		.iter()
		.map(|entry| match entry {
			RailEntry::Guild(id) => json!({
				"id": null,
				"name": null,
				"color": null,
				"guild_ids": [id.to_string()],
			}),
			RailEntry::Folder {
				id,
				name,
				color,
				guilds,
			} => json!({
				"id": id,
				"name": name,
				"color": color,
				"guild_ids": guilds.iter().map(ToString::to_string).collect::<Vec<_>>(),
			}),
		})
		.collect();
	json!({ "guild_folders": folders })
}
