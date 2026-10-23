use std::collections::{HashMap, HashSet};

use protocol::{ProtoGuildFolder, ProtoGuildFolders, Snowflake, UserGuildSettings, decode_guild_folders};
use serde::Deserialize;
use serde_json::{Map, Value, json};

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
	pub fn user_settings(&self) -> &Value {
		&self.user_settings
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

	pub(crate) fn apply_settings_proto(&mut self, encoded: &str) -> Changes {
		let Some(ProtoGuildFolders { folders, positions }) = decode_guild_folders(encoded) else {
			return Changes::default();
		};
		self.guild_folders = folders.into_iter().map(GuildFolder::from).collect();
		self.guild_positions = positions;
		self.reorder_guilds();
		Changes {
			guild_list: true,
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
		match proto {
			Some(encoded) if kind.is_none_or(|kind| kind == 1) => self.apply_settings_proto(encoded),
			_ => Changes::default(),
		}
	}

	pub(crate) fn apply_user_settings(&mut self, settings: &Value) -> Changes {
		let mut changes = Changes::default();
		if let Some(fields) = settings.as_object() {
			if !self.user_settings.is_object() {
				self.user_settings = Value::Object(Map::new());
			}
			if let Some(stored) = self.user_settings.as_object_mut() {
				stored.extend(fields.iter().map(|(key, value)| (key.clone(), value.clone())));
			}
			changes.user_settings = true;
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
