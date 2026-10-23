use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Duration, Utc};
use protocol::{
	Channel, ChannelType, Emoji, LenientVec, Member, PermissionOverwrite, PermissionOverwriteKind,
	Permissions, Relationship, RelationshipKind, Role, Snowflake, UserGuildSettings,
};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::{CURRENT_USER_ID, Fixtures, GuildFixture};
use crate::ids::snowflake_at;
use crate::json::{parse_id, to_json};
use crate::ops::now_iso;
use crate::ready::is_thread;
use crate::state::{Shared, Store};

const AUDIT_PAGE: usize = 50;
const SEED_AUDIT_ENTRIES: i64 = 3;
const MINUTE_MS: i64 = 60_000;
const ACTION_CHANNEL_CREATE: i64 = 10;
const ACTION_CHANNEL_UPDATE: i64 = 11;
const ACTION_CHANNEL_DELETE: i64 = 12;
const ACTION_KICK: i64 = 20;
const ACTION_BAN: i64 = 22;
const ACTION_UNBAN: i64 = 23;
const ACTION_MEMBER_UPDATE: i64 = 24;
const ACTION_ROLE_CREATE: i64 = 30;
const ACTION_ROLE_UPDATE: i64 = 31;
const ACTION_ROLE_DELETE: i64 = 32;
const ACTION_INVITE_CREATE: i64 = 40;
const ACTION_EMOJI_CREATE: i64 = 60;
const ACTION_EMOJI_DELETE: i64 = 62;
const ACTION_GUILD_UPDATE: i64 = 1;
const ACTION_PIN: i64 = 74;
const ACTION_UNPIN: i64 = 75;
const ACTION_BULK_DELETE: i64 = 73;

pub fn seed_audit(fixtures: &Fixtures) -> HashMap<u64, Vec<Value>> {
	let anchor_ms = fixtures.anchor.timestamp_millis();
	fixtures
		.guilds
		.iter()
		.map(|guild| {
			let actor = guild.properties.owner_id.map_or(CURRENT_USER_ID, Snowflake::get);
			let entries = (0..SEED_AUDIT_ENTRIES)
				.map(|step| {
					let (action, name) = match step {
						0 => (ACTION_CHANNEL_CREATE, "announcements"),
						1 => (ACTION_ROLE_UPDATE, "Moderator"),
						_ => (ACTION_GUILD_UPDATE, "Server"),
					};
					json!({
						"id": snowflake_at(anchor_ms - (step + 1) * 60 * MINUTE_MS, 0).to_string(),
						"user_id": actor.to_string(),
						"target_id": null,
						"action_type": action,
						"changes": [{ "key": "name", "new_value": name }],
						"reason": null,
					})
				})
				.collect();
			(guild.id, entries)
		})
		.collect()
}

impl Store {
	pub fn push_audit(
		&mut self,
		guild_id: u64,
		action_type: i64,
		target_id: Option<u64>,
		changes: Value,
		reason: Option<&str>,
	) {
		let id = self.next_snowflake();
		let entry = json!({
			"id": id.to_string(),
			"user_id": CURRENT_USER_ID.to_string(),
			"target_id": target_id.map(|target| target.to_string()),
			"action_type": action_type,
			"changes": changes,
			"reason": reason,
		});
		self.audit.entry(guild_id).or_default().insert(0, entry);
	}

	pub fn member_now(&self, guild: &GuildFixture, user_id: u64) -> Option<Member> {
		if self
			.removed_members
			.get(&guild.id)
			.is_some_and(|removed| removed.contains(&user_id))
		{
			return None;
		}
		self.member_edits
			.get(&(guild.id, user_id))
			.cloned()
			.or_else(|| guild.member(user_id).cloned())
	}

	pub fn live_member_count(&self, guild: &GuildFixture) -> usize {
		guild.members.len() - self.removed_members.get(&guild.id).map_or(0, BTreeSet::len)
	}

	pub fn is_removed(&self, guild_id: u64, user_id: u64) -> bool {
		self.removed_members
			.get(&guild_id)
			.is_some_and(|removed| removed.contains(&user_id))
	}

	pub fn guild_channels(&self, guild: &GuildFixture) -> Vec<&Channel> {
		let mut channels: Vec<&Channel> = guild
			.channel_ids
			.iter()
			.filter_map(|id| self.channels.get(id))
			.collect();
		let mut added: Vec<&Channel> = self
			.channels
			.values()
			.filter(|channel| {
				channel.guild_id == Some(Snowflake(guild.id))
					&& !is_thread(channel.kind)
					&& !guild.channel_ids.contains(&channel.id.0)
			})
			.collect();
		added.sort_by_key(|channel| channel.id);
		channels.extend(added);
		channels
	}

	pub fn thread_json(&self, thread: &Channel) -> Value {
		let mut value = to_json(thread);
		if self.joined_threads.contains(&thread.id.0) {
			value["member"] = json!({
				"id": thread.id.to_string(),
				"user_id": CURRENT_USER_ID.to_string(),
				"join_timestamp": now_iso(),
				"flags": 0,
			});
		}
		value
	}
}

fn guild_payload(store: &Store, guild: &GuildFixture) -> Value {
	let mut payload = to_json(&store.properties[&guild.id]);
	payload["id"] = json!(guild.id.to_string());
	payload["roles"] = to_json(&store.roles[&guild.id]);
	payload["emojis"] = to_json(&store.emojis[&guild.id]);
	payload
}

fn permissions_of(value: &Value) -> Permissions {
	serde_json::from_value(value.clone()).unwrap_or_else(|_| Permissions::empty())
}

fn user_ids(value: &Value) -> Vec<Snowflake> {
	value
		.as_array()
		.map(|ids| ids.iter().filter_map(parse_id).map(Snowflake).collect())
		.unwrap_or_default()
}

impl Shared {
	fn live_guild(&self, store: &Store, id: u64) -> Result<&GuildFixture, ApiError> {
		self.fixtures
			.guild(id)
			.filter(|_| !store.left_guilds.contains(&id))
			.ok_or_else(ApiError::unknown_guild)
	}

	pub fn add_relationship_by_username(&self, username: &str) -> Result<(), ApiError> {
		let user = self
			.fixtures
			.users
			.values()
			.find(|user| user.id.0 != CURRENT_USER_ID && user.username.eq_ignore_ascii_case(username))
			.ok_or_else(ApiError::unknown_user)?;
		self.set_relationship(user.id.0, RelationshipKind::OutgoingRequest)
	}

	pub fn put_relationship(&self, user_id: u64, kind: Option<i64>) -> Result<(), ApiError> {
		let next = match kind.map(RelationshipKind::from) {
			Some(RelationshipKind::Blocked) => RelationshipKind::Blocked,
			_ => match self
				.store
				.lock()
				.relationships
				.get(&user_id)
				.map(|existing| existing.kind)
			{
				Some(RelationshipKind::IncomingRequest | RelationshipKind::Friend) => {
					RelationshipKind::Friend
				}
				_ => RelationshipKind::OutgoingRequest,
			},
		};
		self.set_relationship(user_id, next)
	}

	fn set_relationship(&self, user_id: u64, kind: RelationshipKind) -> Result<(), ApiError> {
		let user = self
			.fixtures
			.users
			.get(&user_id)
			.cloned()
			.ok_or_else(ApiError::unknown_user)?;
		let relationship = Relationship {
			id: user.id,
			kind,
			user: Some(user),
			user_id: None,
			nickname: None,
			since: Some(Utc::now()),
		};
		let event = to_json(&relationship);
		self.store.lock().relationships.insert(user_id, relationship);
		self.hub.broadcast("RELATIONSHIP_ADD", &event);
		Ok(())
	}

	pub fn delete_relationship(&self, user_id: u64) -> Result<(), ApiError> {
		let removed = self
			.store
			.lock()
			.relationships
			.remove(&user_id)
			.ok_or_else(ApiError::unknown_user)?;
		self.hub.broadcast(
			"RELATIONSHIP_REMOVE",
			&json!({ "id": user_id.to_string(), "type": i64::from(removed.kind) }),
		);
		Ok(())
	}

	pub fn patch_guild_settings(&self, scope: u64, body: &Value) -> Value {
		let mut store = self.store.lock();
		let mut settings = store
			.settings
			.get(&scope)
			.cloned()
			.unwrap_or_else(|| UserGuildSettings {
				guild_id: (scope != 0).then_some(Snowflake(scope)),
				..UserGuildSettings::default()
			});
		let mut merged = to_json(&settings);
		if let Some(fields) = body.as_object() {
			for (key, value) in fields
				.iter()
				.filter(|(key, _)| key.as_str() != "channel_overrides")
			{
				merged[key] = value.clone();
			}
		}
		if let Some(overrides) = body["channel_overrides"].as_object() {
			let list = merged["channel_overrides"]
				.as_array_mut()
				.cloned()
				.unwrap_or_default();
			let mut list = list;
			for (channel_key, change) in overrides {
				let existing = list
					.iter_mut()
					.find(|entry| entry["channel_id"].as_str() == Some(channel_key.as_str()));
				match existing {
					Some(entry) => {
						if let Some(fields) = change.as_object() {
							for (key, value) in fields {
								entry[key] = value.clone();
							}
						}
					}
					None => {
						let mut entry = change.clone();
						entry["channel_id"] = json!(channel_key);
						list.push(entry);
					}
				}
			}
			merged["channel_overrides"] = Value::Array(list);
		}
		if let Ok(parsed) = serde_json::from_value::<UserGuildSettings>(merged.clone()) {
			settings = parsed;
		}
		store.settings.insert(scope, settings);
		drop(store);
		self.hub.broadcast("USER_GUILD_SETTINGS_UPDATE", &merged);
		merged
	}

	pub fn modify_guild(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let guild = self.live_guild(&store, guild_id)?;
		let new_icon = format!("{:032x}", store.next_snowflake());
		let properties = store
			.properties
			.get_mut(&guild_id)
			.ok_or_else(ApiError::unknown_guild)?;
		let mut changes = Vec::new();
		if let Some(name) = body["name"].as_str() {
			changes.push(json!({ "key": "name", "old_value": properties.name, "new_value": name }));
			properties.name = Some(name.to_owned());
		}
		if let Some(description) = body["description"].as_str() {
			properties.description = Some(description.to_owned());
		}
		if body["icon"].is_string() {
			properties.icon = Some(new_icon);
		}
		store.push_audit(
			guild_id,
			ACTION_GUILD_UPDATE,
			Some(guild_id),
			Value::Array(changes),
			None,
		);
		let payload = guild_payload(&store, guild);
		drop(store);
		self.hub.broadcast("GUILD_UPDATE", &payload);
		Ok(payload)
	}

	pub fn create_channel(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		let name = body["name"]
			.as_str()
			.filter(|name| !name.is_empty())
			.ok_or_else(|| ApiError::invalid_body("name is required"))?;
		let mut store = self.store.lock();
		let guild = self.live_guild(&store, guild_id)?;
		let position = store
			.guild_channels(guild)
			.iter()
			.filter_map(|channel| channel.position)
			.max()
			.unwrap_or(0)
			+ 1;
		let id = store.next_snowflake();
		let channel = Channel {
			id: Snowflake(id),
			kind: ChannelType::from(body["type"].as_i64().unwrap_or(0)),
			guild_id: Some(Snowflake(guild_id)),
			name: Some(name.to_owned()),
			parent_id: parse_id(&body["parent_id"]).map(Snowflake),
			position: Some(position),
			..Channel::default()
		};
		let value = to_json(&channel);
		store.channels.insert(id, channel);
		store.messages.entry(id).or_default();
		store.push_audit(
			guild_id,
			ACTION_CHANNEL_CREATE,
			Some(id),
			json!([{ "key": "name", "new_value": name }]),
			None,
		);
		drop(store);
		self.hub.broadcast("CHANNEL_CREATE", &value);
		Ok(value)
	}

	pub fn modify_channel(&self, channel_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let channel = store
			.channels
			.get_mut(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?;
		if let Some(name) = body["name"].as_str() {
			channel.name = Some(name.to_owned());
		}
		if let Some(topic) = body["topic"].as_str() {
			channel.topic = Some(topic.to_owned());
		}
		if let Some(nsfw) = body["nsfw"].as_bool() {
			channel.nsfw = Some(nsfw);
		}
		if let Some(slowmode) = body["rate_limit_per_user"].as_i64() {
			channel.rate_limit_per_user = Some(slowmode);
		}
		if let Some(position) = body["position"].as_i64() {
			channel.position = Some(position);
		}
		if body.get("parent_id").is_some() {
			channel.parent_id = parse_id(&body["parent_id"]).map(Snowflake);
		}
		if let Some(metadata) = channel.thread_metadata.as_mut() {
			if let Some(archived) = body["archived"].as_bool() {
				metadata.archived = archived;
				metadata.archive_timestamp = Some(Utc::now());
			}
			if let Some(locked) = body["locked"].as_bool() {
				metadata.locked = locked;
			}
		}
		let thread = is_thread(channel.kind);
		let guild_id = channel.guild_id;
		let value = if thread {
			let channel = channel.clone();
			store.thread_json(&channel)
		} else {
			to_json(&*channel)
		};
		if let Some(guild_id) = guild_id {
			store.push_audit(
				guild_id.0,
				ACTION_CHANNEL_UPDATE,
				Some(channel_id),
				json!([]),
				None,
			);
		}
		drop(store);
		self.hub
			.broadcast(if thread { "THREAD_UPDATE" } else { "CHANNEL_UPDATE" }, &value);
		Ok(value)
	}

	pub fn delete_channel(&self, channel_id: u64) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let channel = store
			.channels
			.remove(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?;
		store.private_channel_ids.retain(|id| *id != channel_id);
		store.messages.remove(&channel_id);
		if let Some(guild_id) = channel.guild_id {
			store.push_audit(
				guild_id.0,
				ACTION_CHANNEL_DELETE,
				Some(channel_id),
				json!([]),
				None,
			);
		}
		drop(store);
		let value = to_json(&channel);
		if is_thread(channel.kind) {
			self.hub.broadcast(
				"THREAD_DELETE",
				&json!({
					"id": channel_id.to_string(),
					"guild_id": channel.guild_id.map(|id| id.to_string()),
					"parent_id": channel.parent_id.map(|id| id.to_string()),
					"type": i64::from(channel.kind),
				}),
			);
		} else {
			self.hub.broadcast("CHANNEL_DELETE", &value);
		}
		Ok(value)
	}

	pub fn set_overwrite(
		&self,
		channel_id: u64,
		target_id: u64,
		body: Option<&Value>,
	) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let channel = store
			.channels
			.get_mut(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?;
		channel
			.permission_overwrites
			.retain(|overwrite| overwrite.id.0 != target_id);
		if let Some(body) = body {
			channel.permission_overwrites.push(PermissionOverwrite {
				id: Snowflake(target_id),
				kind: PermissionOverwriteKind::from(body["type"].as_i64().unwrap_or(0)),
				allow: permissions_of(&body["allow"]),
				deny: permissions_of(&body["deny"]),
			});
		}
		let value = to_json(&*channel);
		drop(store);
		self.hub.broadcast("CHANNEL_UPDATE", &value);
		Ok(())
	}

	pub fn create_role(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let id = store.next_snowflake();
		let role = Role {
			id: Snowflake(id),
			name: body["name"].as_str().unwrap_or("new role").to_owned(),
			color: body["color"].as_i64().unwrap_or(0),
			hoist: body["hoist"].as_bool().unwrap_or(false),
			position: 1,
			permissions: permissions_of(&body["permissions"]),
			mentionable: body["mentionable"].as_bool().unwrap_or(false),
			..Role::default()
		};
		let value = to_json(&role);
		store.roles.entry(guild_id).or_default().push(role);
		store.push_audit(guild_id, ACTION_ROLE_CREATE, Some(id), json!([]), None);
		drop(store);
		self.hub.broadcast(
			"GUILD_ROLE_CREATE",
			&json!({ "guild_id": guild_id.to_string(), "role": value }),
		);
		Ok(value)
	}

	pub fn modify_role(&self, guild_id: u64, role_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let role = store
			.roles
			.get_mut(&guild_id)
			.and_then(|roles| roles.iter_mut().find(|role| role.id.0 == role_id))
			.ok_or_else(ApiError::not_found)?;
		if let Some(name) = body["name"].as_str() {
			role.name = name.to_owned();
		}
		if let Some(color) = body["color"].as_i64() {
			role.color = color;
		}
		if let Some(hoist) = body["hoist"].as_bool() {
			role.hoist = hoist;
		}
		if let Some(mentionable) = body["mentionable"].as_bool() {
			role.mentionable = mentionable;
		}
		if !body["permissions"].is_null() {
			role.permissions = permissions_of(&body["permissions"]);
		}
		let value = to_json(&*role);
		store.push_audit(guild_id, ACTION_ROLE_UPDATE, Some(role_id), json!([]), None);
		drop(store);
		self.hub.broadcast(
			"GUILD_ROLE_UPDATE",
			&json!({ "guild_id": guild_id.to_string(), "role": value }),
		);
		Ok(value)
	}

	pub fn delete_role(&self, guild_id: u64, role_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let roles = store.roles.get_mut(&guild_id).ok_or_else(ApiError::not_found)?;
		let before = roles.len();
		roles.retain(|role| role.id.0 != role_id);
		if roles.len() == before {
			return Err(ApiError::not_found());
		}
		store.push_audit(guild_id, ACTION_ROLE_DELETE, Some(role_id), json!([]), None);
		drop(store);
		self.hub.broadcast(
			"GUILD_ROLE_DELETE",
			&json!({ "guild_id": guild_id.to_string(), "role_id": role_id.to_string() }),
		);
		Ok(())
	}

	pub fn reorder_roles(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let roles = store.roles.get_mut(&guild_id).ok_or_else(ApiError::not_found)?;
		let mut changed = Vec::new();
		for entry in body.as_array().into_iter().flatten() {
			let (Some(id), Some(position)) = (parse_id(&entry["id"]), entry["position"].as_i64()) else {
				continue;
			};
			if let Some(role) = roles.iter_mut().find(|role| role.id.0 == id) {
				role.position = position;
				changed.push(to_json(&*role));
			}
		}
		let all = to_json(&*roles);
		drop(store);
		for role in changed {
			self.hub.broadcast(
				"GUILD_ROLE_UPDATE",
				&json!({ "guild_id": guild_id.to_string(), "role": role }),
			);
		}
		Ok(all)
	}

	pub fn create_emoji(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		let name = body["name"]
			.as_str()
			.filter(|name| !name.is_empty())
			.ok_or_else(|| ApiError::invalid_body("name is required"))?;
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let id = store.next_snowflake();
		let emoji = Emoji {
			id: Some(Snowflake(id)),
			name: Some(name.to_owned()),
			user: Some(store.current_user.clone()),
			require_colons: Some(true),
			managed: Some(false),
			animated: Some(false),
			available: Some(true),
			..Emoji::default()
		};
		let value = to_json(&emoji);
		store.emojis.entry(guild_id).or_default().push(emoji);
		store.push_audit(guild_id, ACTION_EMOJI_CREATE, Some(id), json!([]), None);
		let all = to_json(&store.emojis[&guild_id]);
		drop(store);
		self.hub.broadcast(
			"GUILD_EMOJIS_UPDATE",
			&json!({ "guild_id": guild_id.to_string(), "emojis": all }),
		);
		Ok(value)
	}

	pub fn delete_emoji(&self, guild_id: u64, emoji_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let emojis = store.emojis.get_mut(&guild_id).ok_or_else(ApiError::not_found)?;
		let before = emojis.len();
		emojis.retain(|emoji| emoji.id.map(Snowflake::get) != Some(emoji_id));
		if emojis.len() == before {
			return Err(ApiError::not_found());
		}
		let all = to_json(&*emojis);
		store.push_audit(guild_id, ACTION_EMOJI_DELETE, Some(emoji_id), json!([]), None);
		drop(store);
		self.hub.broadcast(
			"GUILD_EMOJIS_UPDATE",
			&json!({ "guild_id": guild_id.to_string(), "emojis": all }),
		);
		Ok(())
	}

	pub fn modify_member(&self, guild_id: u64, user_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let guild = self.live_guild(&store, guild_id)?;
		let mut member = store
			.member_now(guild, user_id)
			.ok_or_else(ApiError::unknown_member)?;
		if let Some(nick) = body.get("nick") {
			member.nick = nick.as_str().filter(|nick| !nick.is_empty()).map(str::to_owned);
		}
		if body["avatar"].is_string() {
			member.avatar = Some(format!("{:032x}", store.next_snowflake()));
		}
		if body["roles"].is_array() {
			member.roles = user_ids(&body["roles"]);
		}
		if let Some(mute) = body["mute"].as_bool() {
			member.mute = mute;
		}
		if let Some(deaf) = body["deaf"].as_bool() {
			member.deaf = deaf;
		}
		if let Some(until) = body.get("communication_disabled_until") {
			member.communication_disabled_until =
				serde_json::from_value::<Option<DateTime<Utc>>>(until.clone())
					.ok()
					.flatten();
		}
		member.guild_id = Some(Snowflake(guild_id));
		let value = to_json(&member);
		store.member_edits.insert((guild_id, user_id), member);
		store.push_audit(guild_id, ACTION_MEMBER_UPDATE, Some(user_id), json!([]), None);
		drop(store);
		self.hub.broadcast("GUILD_MEMBER_UPDATE", &value);
		Ok(value)
	}

	pub fn set_member_role(
		&self,
		guild_id: u64,
		user_id: u64,
		role_id: u64,
		add: bool,
	) -> Result<(), ApiError> {
		let roles = {
			let store = self.store.lock();
			let guild = self.live_guild(&store, guild_id)?;
			let mut roles = store
				.member_now(guild, user_id)
				.ok_or_else(ApiError::unknown_member)?
				.roles;
			roles.retain(|role| role.0 != role_id);
			if add {
				roles.push(Snowflake(role_id));
			}
			roles
		};
		self.modify_member(guild_id, user_id, &json!({ "roles": roles }))
			.map(drop)
	}

	pub fn drop_member(
		&self,
		guild_id: u64,
		user_id: u64,
		ban: bool,
		reason: Option<&str>,
	) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let guild = self.live_guild(&store, guild_id)?;
		store
			.member_now(guild, user_id)
			.ok_or_else(ApiError::unknown_member)?;
		store.removed_members.entry(guild_id).or_default().insert(user_id);
		if ban {
			store
				.bans
				.entry(guild_id)
				.or_default()
				.insert(user_id, reason.map(str::to_owned));
		}
		store.push_audit(
			guild_id,
			if ban { ACTION_BAN } else { ACTION_KICK },
			Some(user_id),
			json!([]),
			reason,
		);
		drop(store);
		let user = self.fixtures.users.get(&user_id).map_or(Value::Null, to_json);
		self.hub.broadcast(
			"GUILD_MEMBER_REMOVE",
			&json!({ "guild_id": guild_id.to_string(), "user": user }),
		);
		Ok(())
	}

	pub fn unban(&self, guild_id: u64, user_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		store
			.bans
			.get_mut(&guild_id)
			.and_then(|bans| bans.remove(&user_id))
			.ok_or_else(ApiError::unknown_member)?;
		store
			.removed_members
			.entry(guild_id)
			.or_default()
			.remove(&user_id);
		store.push_audit(guild_id, ACTION_UNBAN, Some(user_id), json!([]), None);
		Ok(())
	}

	pub fn bans_json(&self, guild_id: u64) -> Result<Value, ApiError> {
		let store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let bans: Vec<Value> = store
			.bans
			.get(&guild_id)
			.into_iter()
			.flatten()
			.map(|(user_id, reason)| {
				json!({ "user": self.fixtures.users.get(user_id).map_or(Value::Null, to_json), "reason": reason })
			})
			.collect();
		Ok(Value::Array(bans))
	}

	pub fn audit_log_json(
		&self,
		guild_id: u64,
		before: Option<u64>,
		action_type: Option<i64>,
	) -> Result<Value, ApiError> {
		let store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let entries: Vec<&Value> = store
			.audit
			.get(&guild_id)
			.into_iter()
			.flatten()
			.filter(|entry| before.is_none_or(|before| parse_id(&entry["id"]).is_some_and(|id| id < before)))
			.filter(|entry| action_type.is_none_or(|kind| entry["action_type"].as_i64() == Some(kind)))
			.take(AUDIT_PAGE)
			.collect();
		let mut user_ids: BTreeSet<u64> = BTreeSet::new();
		for entry in &entries {
			user_ids.extend(parse_id(&entry["user_id"]));
			user_ids.extend(parse_id(&entry["target_id"]));
		}
		let users: Vec<Value> = user_ids
			.iter()
			.filter_map(|id| self.fixtures.users.get(id))
			.map(to_json)
			.collect();
		Ok(json!({
			"audit_log_entries": entries,
			"users": users,
			"webhooks": [],
			"integrations": [],
			"threads": [],
			"application_commands": [],
		}))
	}

	fn invite_value(&self, store: &Store, guild: &GuildFixture, channel: Option<&Channel>) -> Value {
		let properties = &store.properties[&guild.id];
		json!({
			"type": 0,
			"guild_id": guild.id.to_string(),
			"guild": {
				"id": guild.id.to_string(),
				"name": properties.name,
				"icon": properties.icon,
				"splash": null,
				"banner": null,
				"description": properties.description,
				"features": properties.features,
				"verification_level": properties.verification_level,
				"vanity_url_code": null,
				"nsfw_level": properties.nsfw_level,
				"premium_subscription_count": properties.premium_subscription_count,
			},
			"channel": channel.map(|channel| json!({
				"id": channel.id.to_string(),
				"type": channel.kind,
				"name": channel.name,
			})),
			"inviter": store.current_user,
			"approximate_member_count": store.live_member_count(guild),
			"approximate_presence_count": store.online_count(guild.id),
		})
	}

	pub fn create_invite(&self, channel_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let channel = store
			.channels
			.get(&channel_id)
			.cloned()
			.ok_or_else(ApiError::unknown_channel)?;
		let guild_id = channel.guild_id.ok_or_else(ApiError::wrong_channel_kind)?.0;
		let guild = self.live_guild(&store, guild_id)?;
		let code = format!("mockinvite{}", store.invites.len() + 1);
		let max_age = body["max_age"].as_i64().unwrap_or(86_400);
		let mut value = self.invite_value(&store, guild, Some(&channel));
		value["code"] = json!(code);
		value["uses"] = json!(0);
		value["max_uses"] = json!(body["max_uses"].as_i64().unwrap_or(0));
		value["max_age"] = json!(max_age);
		value["temporary"] = json!(body["temporary"].as_bool().unwrap_or(false));
		value["created_at"] = json!(now_iso());
		value["expires_at"] = if max_age > 0 {
			json!((Utc::now() + Duration::seconds(max_age)).to_rfc3339())
		} else {
			Value::Null
		};
		store.invites.insert(code.clone(), value.clone());
		store.push_audit(
			guild_id,
			ACTION_INVITE_CREATE,
			None,
			json!([{ "key": "code", "new_value": code }]),
			None,
		);
		Ok(value)
	}

	pub fn invite_json(&self, code: &str) -> Option<Value> {
		let store = self.store.lock();
		let mut value = store.invites.get(code)?.clone();
		let guild_id = parse_id(&value["guild_id"])?;
		let guild = self.live_guild(&store, guild_id).ok()?;
		value["approximate_member_count"] = json!(store.live_member_count(guild));
		value["approximate_presence_count"] = json!(store.online_count(guild_id));
		Some(value)
	}

	pub fn create_guild(&self, body: &Value) -> Result<Value, ApiError> {
		let name = body["name"]
			.as_str()
			.filter(|name| !name.is_empty())
			.ok_or_else(|| ApiError::invalid_body("name is required"))?;
		let mut store = self.store.lock();
		let guild_id = store.next_snowflake();
		let channel_id = store.next_snowflake();
		let owner = to_json(&store.current_user);
		let joined = Utc::now().to_rfc3339();
		let guild = json!({
			"id": guild_id.to_string(),
			"name": name,
			"icon": Value::Null,
			"owner_id": CURRENT_USER_ID.to_string(),
			"member_count": 1,
			"joined_at": joined,
			"roles": [{ "id": guild_id.to_string(), "name": "@everyone", "permissions": "2248473465835073", "position": 0, "color": 0 }],
			"emojis": [],
			"stickers": [],
			"channels": [{
				"id": channel_id.to_string(),
				"type": 0,
				"guild_id": guild_id.to_string(),
				"name": "general",
				"position": 0,
			}],
			"threads": [],
			"members": [{ "user": owner, "roles": [], "joined_at": joined }],
			"voice_states": [],
			"presences": [],
			"features": [],
			"premium_tier": 0,
			"verification_level": 0,
			"system_channel_id": channel_id.to_string(),
		});
		store.created_guilds.insert(guild_id, guild.clone());
		drop(store);
		self.hub.broadcast("GUILD_CREATE", &guild);
		Ok(guild)
	}

	pub fn delete_invite(&self, code: &str) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let invite = store.invites.remove(code).ok_or_else(ApiError::unknown_invite)?;
		drop(store);
		self.hub.broadcast("INVITE_DELETE", &invite);
		Ok(invite)
	}

	pub fn guild_invites_json(&self, guild_id: u64) -> Result<Value, ApiError> {
		let store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let invites: Vec<&Value> = store
			.invites
			.values()
			.filter(|invite| parse_id(&invite["guild_id"]) == Some(guild_id))
			.collect();
		Ok(json!(invites))
	}

	pub fn leave_guild(&self, guild_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		store.left_guilds.insert(guild_id);
		drop(store);
		self.hub
			.broadcast("GUILD_DELETE", &json!({ "id": guild_id.to_string() }));
		Ok(())
	}

	pub fn group_recipient(&self, channel_id: u64, user_id: u64, add: bool) -> Result<(), ApiError> {
		let user = self
			.fixtures
			.users
			.get(&user_id)
			.cloned()
			.ok_or_else(ApiError::unknown_user)?;
		let mut store = self.store.lock();
		let channel = store
			.channels
			.get_mut(&channel_id)
			.filter(|channel| channel.kind == ChannelType::GroupDm)
			.ok_or_else(ApiError::wrong_channel_kind)?;
		channel.recipient_ids.retain(|id| id.0 != user_id);
		let mut recipients: Vec<_> = std::mem::take(&mut channel.recipients).into_vec();
		recipients.retain(|recipient| recipient.id.0 != user_id);
		if add {
			channel.recipient_ids.push(Snowflake(user_id));
			recipients.push(user);
		}
		channel.recipients = LenientVec::from(recipients);
		let value = to_json(&*channel);
		drop(store);
		self.hub.broadcast("CHANNEL_UPDATE", &value);
		Ok(())
	}

	pub fn thread_membership(&self, thread_id: u64, join: bool) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let thread = store
			.channels
			.get_mut(&thread_id)
			.filter(|channel| is_thread(channel.kind))
			.ok_or_else(ApiError::wrong_channel_kind)?;
		let count = thread.member_count.unwrap_or(0) + if join { 1 } else { -1 };
		thread.member_count = Some(count.max(0));
		let guild_id = thread.guild_id.map(|id| id.to_string());
		if join {
			store.joined_threads.insert(thread_id);
		} else {
			store.joined_threads.remove(&thread_id);
		}
		drop(store);
		let mut update = json!({
			"id": thread_id.to_string(),
			"guild_id": guild_id,
			"member_count": count.max(0),
		});
		if join {
			update["added_members"] = json!([{
				"id": thread_id.to_string(),
				"user_id": CURRENT_USER_ID.to_string(),
				"join_timestamp": now_iso(),
				"flags": 0,
			}]);
		} else {
			update["removed_member_ids"] = json!([CURRENT_USER_ID.to_string()]);
		}
		self.hub.broadcast("THREAD_MEMBERS_UPDATE", &update);
		Ok(())
	}

	pub fn threads_json(&self, filter: impl Fn(&Channel) -> bool) -> Value {
		let store = self.store.lock();
		let mut threads: Vec<&Channel> = store
			.channels
			.values()
			.filter(|channel| is_thread(channel.kind) && filter(channel))
			.collect();
		threads.sort_by_key(|channel| std::cmp::Reverse(channel.id));
		let members: Vec<Value> = threads
			.iter()
			.filter(|thread| store.joined_threads.contains(&thread.id.0))
			.map(|thread| {
				json!({
					"id": thread.id.to_string(),
					"user_id": CURRENT_USER_ID.to_string(),
					"join_timestamp": now_iso(),
					"flags": 0,
				})
			})
			.collect();
		json!({
			"threads": threads.iter().map(|thread| store.thread_json(thread)).collect::<Vec<_>>(),
			"members": members,
			"has_more": false,
		})
	}

	pub fn set_pin(&self, channel_id: u64, message_id: u64, pinned: bool) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let guild_id = store
			.channels
			.get(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?
			.guild_id;
		let message = store
			.message_mut(channel_id, message_id)
			.ok_or_else(ApiError::unknown_message)?;
		message["pinned"] = json!(pinned);
		let value = message.clone();
		let gateway = self.gateway_message(&store, &value);
		if let Some(guild_id) = guild_id {
			store.push_audit(
				guild_id.0,
				if pinned { ACTION_PIN } else { ACTION_UNPIN },
				Some(message_id),
				json!([]),
				None,
			);
		}
		drop(store);
		self.hub.broadcast("MESSAGE_UPDATE", &gateway);
		self.hub.broadcast(
			"CHANNEL_PINS_UPDATE",
			&json!({
				"guild_id": guild_id.map(|id| id.to_string()),
				"channel_id": channel_id.to_string(),
				"last_pin_timestamp": now_iso(),
			}),
		);
		Ok(())
	}

	pub fn bulk_delete(&self, channel_id: u64, ids: &[u64]) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let guild_id = store
			.channels
			.get(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?
			.guild_id;
		let messages = store.messages.entry(channel_id).or_default();
		let removed: Vec<String> = ids
			.iter()
			.filter(|id| messages.remove(id).is_some())
			.map(u64::to_string)
			.collect();
		store.refresh_last_message(channel_id);
		if let Some(guild_id) = guild_id {
			store.push_audit(guild_id.0, ACTION_BULK_DELETE, None, json!([]), None);
		}
		drop(store);
		self.hub.broadcast(
			"MESSAGE_DELETE_BULK",
			&json!({
				"ids": removed,
				"channel_id": channel_id.to_string(),
				"guild_id": guild_id.map(|id| id.to_string()),
			}),
		);
		Ok(())
	}

	pub fn vote_poll(
		&self,
		channel_id: u64,
		message_id: u64,
		answers: &[i64],
		finalize: bool,
	) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let message = store
			.message_mut(channel_id, message_id)
			.ok_or_else(ApiError::unknown_message)?;
		if message["poll"].is_null() {
			return Err(ApiError::invalid_body("This message has no poll"));
		}
		let results = &mut message["poll"]["results"];
		if !results.is_object() {
			*results = json!({ "is_finalized": false, "answer_counts": [] });
		}
		if finalize {
			results["is_finalized"] = json!(true);
		} else {
			let counts = results["answer_counts"]
				.as_array_mut()
				.ok_or_else(ApiError::unknown_message)?;
			for count in counts
				.iter_mut()
				.filter(|count| count["me_voted"].as_bool() == Some(true))
			{
				count["count"] = json!(count["count"].as_i64().unwrap_or(1) - 1);
				count["me_voted"] = json!(false);
			}
			for answer in answers {
				match counts
					.iter_mut()
					.find(|count| count["id"].as_i64() == Some(*answer))
				{
					Some(count) => {
						count["count"] = json!(count["count"].as_i64().unwrap_or(0) + 1);
						count["me_voted"] = json!(true);
					}
					None => counts.push(json!({ "id": answer, "count": 1, "me_voted": true })),
				}
			}
		}
		let value = message.clone();
		let gateway = self.gateway_message(&store, &value);
		drop(store);
		self.hub.broadcast("MESSAGE_UPDATE", &gateway);
		Ok(value)
	}

	pub fn reaction_users(
		&self,
		channel_id: u64,
		message_id: u64,
		emoji: &str,
		limit: usize,
	) -> Result<Value, ApiError> {
		let store = self.store.lock();
		let message = store
			.message(channel_id, message_id)
			.ok_or_else(ApiError::unknown_message)?;
		let name = emoji.rsplit_once(':').map_or(emoji, |(name, _)| name);
		let count = message["reactions"]
			.as_array()
			.into_iter()
			.flatten()
			.find(|reaction| reaction["emoji"]["name"].as_str() == Some(name))
			.and_then(|reaction| reaction["count"].as_u64())
			.ok_or_else(ApiError::unknown_message)? as usize;
		let users: Vec<Value> = self
			.fixtures
			.users
			.values()
			.take(count.min(limit))
			.map(to_json)
			.collect();
		Ok(Value::Array(users))
	}

	pub fn patch_current_user(&self, body: &Value) -> Value {
		let mut store = self.store.lock();
		if let Some(name) = body["global_name"].as_str() {
			store.current_user.global_name = Some(name.to_owned()).filter(|name| !name.is_empty());
		}
		if body["avatar"].is_string() {
			store.current_user.avatar = Some(format!("{:032x}", store.next_snowflake()));
		}
		let value = to_json(&store.current_user);
		drop(store);
		self.hub.broadcast("USER_UPDATE", &value);
		value
	}

	pub fn patch_current_profile(&self, body: &Value) -> Value {
		let mut store = self.store.lock();
		if let Some(fields) = body.as_object() {
			for (key, value) in fields {
				store.profile.insert(key.clone(), value.clone());
			}
		}
		Value::Object(store.profile.clone())
	}
}
