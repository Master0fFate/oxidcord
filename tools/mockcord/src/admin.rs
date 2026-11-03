use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Duration, Utc};
use protocol::{
	Channel, ChannelFlags, ChannelType, DefaultMessageNotifications, Emoji, ForumLayout, ForumSortOrder,
	ForumTag, LenientVec, Member, MessageType, PermissionOverwrite, PermissionOverwriteKind, Permissions,
	Relationship, RelationshipKind, Role, RoleColors, Snowflake, UserGuildSettings, VideoQualityMode,
};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::{CURRENT_USER_ID, Fixtures, GuildFixture, GuildOrigin};
use crate::ids::snowflake_at;
use crate::json::{parse_id, to_json};
use crate::ops::{Draft, now_iso};
use crate::ready::{guild_create, is_thread};
use crate::state::{Shared, Store};

const AUDIT_PAGE: usize = 50;
const MAX_VOICE_STATUS: usize = 500;
const VOICE_SESSION_AGE_SECONDS: i64 = 600;
const MINUTE_MS: i64 = 60_000;
const MAX_TIMEOUT_DAYS: i64 = 28;
const BAN_PAGE: usize = 1000;
const SEED_BANS: u64 = 1100;
const SEED_BAN_FIRST_ID: u64 = 90_000_000_000_000_000;
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
const ACTION_CHANNEL_POSITION: i64 = 16;
const ACTION_ROLE_POSITION: i64 = 33;
const ACTION_AUTOMOD_BLOCK: i64 = 143;
const ACTION_VOICE_STATUS: i64 = 192;
const ACTION_INVITE_CREATE: i64 = 40;
const ACTION_EMOJI_CREATE: i64 = 60;
const ACTION_EMOJI_DELETE: i64 = 62;
const ACTION_GUILD_UPDATE: i64 = 1;
const ACTION_PIN: i64 = 74;
const ACTION_UNPIN: i64 = 75;
pub const ACTION_AUTOMOD_RULE_CREATE: i64 = 140;
pub const ACTION_AUTOMOD_RULE_UPDATE: i64 = 141;
pub const ACTION_AUTOMOD_RULE_DELETE: i64 = 142;

pub fn current_session_hash() -> String {
	format!("mock-session-{CURRENT_USER_ID}")
}

fn apply_role_style(role: &mut Role, body: &Value, icon: Option<String>) {
	if let Some(color) = body["color"].as_i64() {
		role.color = color;
		role.colors = Some(RoleColors {
			primary_color: Some(color),
			..RoleColors::default()
		});
	}
	if body["colors"].is_object() {
		let colors = RoleColors {
			primary_color: body["colors"]["primary_color"].as_i64(),
			secondary_color: body["colors"]["secondary_color"].as_i64(),
			tertiary_color: body["colors"]["tertiary_color"].as_i64(),
		};
		role.color = colors.primary_color.unwrap_or(0);
		role.colors = Some(colors);
	}
	if body.get("icon").is_some() {
		role.icon = icon;
	}
	if let Some(emoji) = body.get("unicode_emoji") {
		role.unicode_emoji = emoji.as_str().map(str::to_owned);
	}
}

pub fn seed_bans(fixtures: &Fixtures) -> HashMap<u64, BTreeMap<u64, Option<String>>> {
	fixtures
		.guilds
		.get(1)
		.map(|guild| {
			let bans = (0..SEED_BANS)
				.map(|offset| {
					(
						SEED_BAN_FIRST_ID + offset,
						(offset % 3 == 0).then(|| "spam".to_owned()),
					)
				})
				.collect();
			(guild.id, bans)
		})
		.into_iter()
		.collect()
}

const SEED_AUDIT: [(i64, &str); 7] = [
	(ACTION_CHANNEL_CREATE, "announcements"),
	(ACTION_ROLE_UPDATE, "Moderator"),
	(ACTION_GUILD_UPDATE, "Server"),
	(ACTION_CHANNEL_POSITION, "channels"),
	(ACTION_ROLE_POSITION, "roles"),
	(ACTION_AUTOMOD_BLOCK, "someone"),
	(ACTION_VOICE_STATUS, "lobby"),
];
const SEED_ROLE_CHANGES: [&str; 7] = [
	"name",
	"color",
	"hoist",
	"mentionable",
	"permissions",
	"icon",
	"unicode_emoji",
];

fn seed_changes(action: i64, name: &str) -> Value {
	if action == ACTION_ROLE_UPDATE {
		return Value::Array(
			SEED_ROLE_CHANGES
				.iter()
				.map(|key| json!({ "key": key, "old_value": "previous", "new_value": name }))
				.collect(),
		);
	}
	json!([{ "key": "name", "new_value": name }])
}

pub fn seed_audit(fixtures: &Fixtures) -> HashMap<u64, Vec<Value>> {
	let anchor_ms = fixtures.anchor.timestamp_millis();
	fixtures
		.guilds
		.iter()
		.map(|guild| {
			let actor = guild.properties.owner_id.map_or(CURRENT_USER_ID, Snowflake::get);
			let entries = (0_i64..)
				.zip(SEED_AUDIT)
				.map(|(step, (action, name))| {
					json!({
						"id": snowflake_at(anchor_ms - (step + 1) * 60 * MINUTE_MS, 0).to_string(),
						"user_id": actor.to_string(),
						"target_id": null,
						"action_type": action,
						"changes": seed_changes(action, name),
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
			value["member"] = self.thread_member_json(thread.id.0);
		}
		value
	}

	pub fn thread_member_json(&self, thread_id: u64) -> Value {
		let mut member = json!({
			"id": thread_id.to_string(),
			"user_id": CURRENT_USER_ID.to_string(),
			"join_timestamp": now_iso(),
			"flags": 0,
		});
		if let Some(Value::Object(settings)) = self.thread_member_settings.get(&thread_id) {
			for (key, value) in settings {
				member[key] = value.clone();
			}
		}
		member
	}

	pub fn thread_visible(&self, thread: &Channel) -> bool {
		thread.kind != ChannelType::PrivateThread || self.joined_threads.contains(&thread.id.0)
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

fn tags_of(value: &Value, mut next_id: impl FnMut() -> u64) -> LenientVec<ForumTag> {
	value
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|entry| {
			let mut tag: ForumTag = serde_json::from_value(entry.clone()).ok()?;
			if tag.id == Snowflake::default() {
				tag.id = Snowflake(next_id());
			}
			Some(tag)
		})
		.collect::<Vec<_>>()
		.into()
}

fn overwrites_of(value: &Value) -> LenientVec<PermissionOverwrite> {
	serde_json::from_value(value.clone()).unwrap_or_default()
}

fn poll_results(message: &mut Value) -> Result<&mut Value, ApiError> {
	if message["poll"].is_null() {
		return Err(ApiError::invalid_body("This message has no poll"));
	}
	let results = &mut message["poll"]["results"];
	if !results.is_object() {
		*results = json!({ "is_finalized": false, "answer_counts": [] });
	}
	Ok(results)
}

fn answer_counts(results: &mut Value) -> Result<&mut Vec<Value>, ApiError> {
	results["answer_counts"]
		.as_array_mut()
		.ok_or_else(ApiError::unknown_message)
}

fn adjust_answer(counts: &mut Vec<Value>, answer_id: i64, delta: i64, mine: bool) {
	let position = counts
		.iter()
		.position(|count| count["id"].as_i64() == Some(answer_id))
		.unwrap_or_else(|| {
			counts.push(json!({ "id": answer_id, "count": 0, "me_voted": false }));
			counts.len() - 1
		});
	let count = &mut counts[position];
	count["count"] = json!((count["count"].as_i64().unwrap_or(0) + delta).max(0));
	if mine {
		count["me_voted"] = json!(delta > 0);
	}
}

fn user_ids(value: &Value) -> Vec<Snowflake> {
	value
		.as_array()
		.map(|ids| ids.iter().filter_map(parse_id).map(Snowflake).collect())
		.unwrap_or_default()
}

impl Shared {
	pub fn auth_sessions_json(&self) -> Value {
		let now = Utc::now();
		let ended = self.store.lock().ended_sessions.clone();
		let sessions = [
			(
				current_session_hash(),
				"windows",
				"desktop",
				"Mock",
				Duration::minutes(2),
			),
			(
				"mock-session-phone".to_owned(),
				"ios",
				"mobile",
				"Mock City",
				Duration::minutes(1),
			),
			(
				"mock-session-browser".to_owned(),
				"linux",
				"web",
				"Mock Town",
				Duration::days(1),
			),
		];
		let user_sessions: Vec<Value> = sessions
			.into_iter()
			.filter(|(hash, ..)| !ended.contains(hash))
			.map(|(hash, os, platform, location, age)| {
				json!({
					"id_hash": hash,
					"approx_last_used_time": (now - age).to_rfc3339(),
					"client_info": { "os": os, "platform": platform, "location": location },
				})
			})
			.collect();
		json!({ "user_sessions": user_sessions })
	}

	pub fn end_sessions(&self, hashes: &Value) {
		let ended = hashes.as_array().into_iter().flatten().filter_map(Value::as_str);
		self.store.lock().ended_sessions.extend(ended.map(str::to_owned));
	}

	pub(crate) fn live_guild(&self, store: &Store, id: u64) -> Result<&GuildFixture, ApiError> {
		self.fixtures
			.guild(id)
			.filter(|_| !store.absent_guilds.contains(&id))
			.ok_or_else(ApiError::unknown_guild)
	}

	pub fn add_relationship_by_username(&self, username: &str) -> Result<(), ApiError> {
		let user = self
			.fixtures
			.users
			.values()
			.find(|user| user.id.0 != CURRENT_USER_ID && user.username.eq_ignore_ascii_case(username))
			.ok_or_else(ApiError::unknown_tag)?;
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
			user_ignored: false,
			is_spam_request: false,
			stranger_request: false,
			should_notify: false,
		};
		let event = to_json(&relationship);
		self.store.lock().relationships.insert(user_id, relationship);
		self.hub.broadcast("RELATIONSHIP_ADD", &event);
		Ok(())
	}

	pub fn incoming_request(&self) -> Result<(), ApiError> {
		let user = {
			let store = self.store.lock();
			self.fixtures
				.users
				.values()
				.find(|user| {
					user.id.0 != CURRENT_USER_ID
						&& !user.bot.unwrap_or(false)
						&& !store.relationships.contains_key(&user.id.0)
				})
				.cloned()
				.ok_or_else(ApiError::unknown_user)?
		};
		let relationship = Relationship {
			id: user.id,
			kind: RelationshipKind::IncomingRequest,
			user: Some(user),
			since: Some(Utc::now()),
			should_notify: true,
			..Relationship::default()
		};
		let event = to_json(&relationship);
		self.store
			.lock()
			.relationships
			.insert(relationship.id.0, relationship);
		self.hub.broadcast("RELATIONSHIP_ADD", &event);
		Ok(())
	}

	pub fn set_ignored(&self, user_id: u64, ignored: bool) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let Some(relationship) = store.relationships.get_mut(&user_id) else {
			drop(store);
			return self.ignore_stranger(user_id, ignored);
		};
		relationship.user_ignored = ignored;
		let kind = relationship.kind;
		if kind == RelationshipKind::None && !ignored {
			store.relationships.remove(&user_id);
			drop(store);
			self.hub.broadcast(
				"RELATIONSHIP_REMOVE",
				&json!({ "id": user_id.to_string(), "type": 0 }),
			);
			return Ok(());
		}
		drop(store);
		self.hub.broadcast(
			"RELATIONSHIP_UPDATE",
			&json!({ "id": user_id.to_string(), "type": i64::from(kind), "user_ignored": ignored }),
		);
		Ok(())
	}

	fn ignore_stranger(&self, user_id: u64, ignored: bool) -> Result<(), ApiError> {
		let user = self
			.fixtures
			.users
			.get(&user_id)
			.cloned()
			.ok_or_else(ApiError::unknown_user)?;
		if !ignored {
			return Ok(());
		}
		let relationship = Relationship {
			id: user.id,
			user: Some(user),
			user_ignored: true,
			..Relationship::default()
		};
		let event = to_json(&relationship);
		self.store.lock().relationships.insert(user_id, relationship);
		self.hub.broadcast("RELATIONSHIP_ADD", &event);
		Ok(())
	}

	pub fn set_friend_nickname(&self, user_id: u64, nickname: Option<String>) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let relationship = store
			.relationships
			.get_mut(&user_id)
			.filter(|relationship| relationship.kind == RelationshipKind::Friend)
			.ok_or_else(ApiError::unknown_user)?;
		relationship.nickname.clone_from(&nickname);
		drop(store);
		self.hub.broadcast(
			"RELATIONSHIP_UPDATE",
			&json!({ "id": user_id.to_string(), "type": 1, "nickname": nickname }),
		);
		Ok(())
	}

	pub fn note(&self, user_id: u64) -> Result<Value, ApiError> {
		let note = self
			.store
			.lock()
			.notes
			.get(&user_id)
			.cloned()
			.ok_or_else(ApiError::not_found)?;
		Ok(json!({
			"user_id": CURRENT_USER_ID.to_string(),
			"note_user_id": user_id.to_string(),
			"note": note,
		}))
	}

	pub fn set_note(&self, user_id: u64, note: &str) -> Result<(), ApiError> {
		if !self.fixtures.users.contains_key(&user_id) {
			return Err(ApiError::unknown_user());
		}
		{
			let mut store = self.store.lock();
			if note.is_empty() {
				store.notes.remove(&user_id);
			} else {
				store.notes.insert(user_id, note.to_owned());
			}
		}
		self.hub.broadcast(
			"USER_NOTE_UPDATE",
			&json!({ "id": user_id.to_string(), "note": note }),
		);
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
			properties.icon = Some(new_icon.clone());
		}
		if body["banner"].is_string() {
			properties.banner = Some(new_icon.clone());
		}
		if body["splash"].is_string() {
			properties.splash = Some(new_icon);
		}
		if body.get("system_channel_id").is_some() {
			properties.system_channel_id = parse_id(&body["system_channel_id"]).map(Snowflake);
		}
		if let Ok(flags) = serde_json::from_value(body["system_channel_flags"].clone()) {
			properties.system_channel_flags = Some(flags);
		}
		if body.get("afk_channel_id").is_some() {
			properties.afk_channel_id = parse_id(&body["afk_channel_id"]).map(Snowflake);
		}
		if let Some(timeout) = body["afk_timeout"].as_i64() {
			properties.afk_timeout = Some(timeout);
		}
		if let Some(level) = body["default_message_notifications"].as_i64() {
			properties.default_message_notifications = Some(DefaultMessageNotifications::from(level));
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
			permission_overwrites: overwrites_of(&body["permission_overwrites"]),
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
		let new_icon = body["icon"]
			.is_string()
			.then(|| format!("{:032x}", store.next_snowflake()));
		let available_tags = body
			.get("available_tags")
			.map(|tags| tags_of(tags, || store.next_snowflake()));
		let channel = store
			.channels
			.get_mut(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?;
		if let Some(name) = body["name"].as_str() {
			channel.name = Some(name.to_owned());
		}
		if let Some(tags) = available_tags {
			channel.available_tags = tags;
		}
		if let Some(applied) = body["applied_tags"].as_array() {
			channel.applied_tags = applied.iter().filter_map(parse_id).map(Snowflake).collect();
		}
		if body.get("default_reaction_emoji").is_some() {
			channel.default_reaction_emoji =
				serde_json::from_value(body["default_reaction_emoji"].clone()).ok();
		}
		if let Some(order) = body["default_sort_order"].as_i64() {
			channel.default_sort_order = Some(ForumSortOrder::from(order));
		}
		if let Some(layout) = body["default_forum_layout"].as_i64() {
			channel.default_forum_layout = Some(ForumLayout::from(layout));
		}
		if let Some(limit) = body["default_thread_rate_limit_per_user"].as_i64() {
			channel.default_thread_rate_limit_per_user = Some(limit);
		}
		if let Ok(flags) = serde_json::from_value(body["flags"].clone()) {
			channel.flags = flags;
		}
		if new_icon.is_some() {
			channel.icon = new_icon;
		}
		if let Some(owner_id) = parse_id(&body["owner"]) {
			channel.owner_id = Some(Snowflake(owner_id));
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
		if let Some(bitrate) = body["bitrate"].as_i64() {
			channel.bitrate = Some(bitrate);
		}
		if let Some(limit) = body["user_limit"].as_i64() {
			channel.user_limit = Some(limit);
		}
		if body.get("rtc_region").is_some() {
			channel.rtc_region = body["rtc_region"].as_str().map(str::to_owned);
		}
		if let Some(mode) = body["video_quality_mode"].as_i64() {
			channel.video_quality_mode = Some(VideoQualityMode::from(mode));
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

	pub fn set_voice_status(&self, channel_id: u64, body: &Value) -> Result<(), ApiError> {
		let status = body["status"].as_str().filter(|status| !status.is_empty());
		if status.is_some_and(|status| status.chars().count() > MAX_VOICE_STATUS) {
			return Err(ApiError::invalid_body("status is too long"));
		}
		let mut store = self.store.lock();
		let channel = store
			.channels
			.get_mut(&channel_id)
			.filter(|channel| channel.kind == ChannelType::GuildVoice)
			.ok_or_else(ApiError::unknown_channel)?;
		channel.status = status.map(str::to_owned);
		let guild_id = channel.guild_id.ok_or_else(ApiError::unknown_channel)?;
		drop(store);
		self.hub.broadcast(
			"VOICE_CHANNEL_STATUS_UPDATE",
			&json!({
				"id": channel_id.to_string(),
				"guild_id": guild_id.to_string(),
				"status": status,
			}),
		);
		Ok(())
	}

	pub fn channel_info(&self, data: &Value) -> Option<Value> {
		let guild_id = parse_id(&data["guild_id"])?;
		let guild = self.fixtures.guild(guild_id)?;
		let wants = |field: &str| {
			data["fields"]
				.as_array()
				.is_some_and(|fields| fields.iter().any(|name| name.as_str() == Some(field)))
		};
		let started = Utc::now().timestamp() - VOICE_SESSION_AGE_SECONDS;
		let store = self.store.lock();
		let mut voice: Vec<&Channel> = store
			.channels
			.values()
			.filter(|channel| {
				channel.guild_id == Some(Snowflake(guild_id)) && channel.kind == ChannelType::GuildVoice
			})
			.collect();
		voice.sort_by_key(|channel| channel.id);
		let channels: Vec<Value> = voice
			.into_iter()
			.map(|channel| {
				let occupied = guild
					.voice_states
					.iter()
					.any(|state| state.channel_id == Some(channel.id));
				let mut entry = json!({ "id": channel.id.to_string() });
				if wants("status") {
					entry["status"] = json!(channel.status);
				}
				if wants("voice_start_time") {
					entry["voice_start_time"] = json!(occupied.then_some(started));
				}
				entry
			})
			.collect();
		Some(json!({ "guild_id": guild_id.to_string(), "channels": channels }))
	}

	pub fn reorder_channels(&self, guild_id: u64, body: &Value) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let mut moved = Vec::new();
		for entry in body.as_array().into_iter().flatten() {
			let (Some(id), Some(position)) = (parse_id(&entry["id"]), entry["position"].as_i64()) else {
				continue;
			};
			let Some(channel) = store
				.channels
				.get_mut(&id)
				.filter(|channel| channel.guild_id == Some(Snowflake(guild_id)))
			else {
				continue;
			};
			channel.position = Some(position);
			if entry.get("parent_id").is_some() {
				channel.parent_id = parse_id(&entry["parent_id"]).map(Snowflake);
			}
			moved.push(to_json(&*channel));
		}
		drop(store);
		for channel in moved {
			self.hub.broadcast("CHANNEL_UPDATE", &channel);
		}
		Ok(())
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

	pub fn accept_message_request(&self, channel_id: u64) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let channel = store
			.channels
			.get_mut(&channel_id)
			.filter(|channel| channel.is_request())
			.ok_or_else(ApiError::unknown_channel)?;
		channel.is_message_request = Some(false);
		channel.is_spam = Some(false);
		channel.flags.remove(ChannelFlags::IS_SPAM);
		let value = to_json(&*channel);
		drop(store);
		self.hub.broadcast("CHANNEL_UPDATE", &value);
		Ok(value)
	}

	pub fn reject_message_requests(&self, channel_ids: &[u64]) -> Result<Vec<Value>, ApiError> {
		{
			let store = self.store.lock();
			let all_pending = channel_ids
				.iter()
				.all(|id| store.channels.get(id).is_some_and(Channel::is_request));
			if !all_pending {
				return Err(ApiError::unknown_channel());
			}
		}
		channel_ids.iter().map(|id| self.delete_channel(*id)).collect()
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
		let icon = body["icon"]
			.is_string()
			.then(|| format!("{:032x}", store.next_snowflake()));
		let mut role = Role {
			id: Snowflake(id),
			name: body["name"].as_str().unwrap_or("new role").to_owned(),
			hoist: body["hoist"].as_bool().unwrap_or(false),
			position: 1,
			permissions: permissions_of(&body["permissions"]),
			mentionable: body["mentionable"].as_bool().unwrap_or(false),
			..Role::default()
		};
		apply_role_style(&mut role, body, icon);
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
		let icon = body["icon"]
			.is_string()
			.then(|| format!("{:032x}", store.next_snowflake()));
		let role = store
			.roles
			.get_mut(&guild_id)
			.and_then(|roles| roles.iter_mut().find(|role| role.id.0 == role_id))
			.ok_or_else(ApiError::not_found)?;
		if let Some(name) = body["name"].as_str() {
			role.name = name.to_owned();
		}
		apply_role_style(role, body, icon);
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

	pub fn role_member_counts(&self, guild_id: u64) -> Result<Value, ApiError> {
		let store = self.store.lock();
		let guild = self.live_guild(&store, guild_id)?;
		let mut counts: HashMap<u64, usize> = HashMap::new();
		for user in guild.members.iter().filter_map(|member| member.user.as_ref()) {
			if let Some(member) = store.member_now(guild, user.id.0) {
				for role in member.roles {
					*counts.entry(role.0).or_default() += 1;
				}
			}
		}
		Ok(Value::Object(
			counts
				.into_iter()
				.map(|(role_id, count)| (role_id.to_string(), json!(count)))
				.collect(),
		))
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
		let target_admin = guild
			.permissions_for(&member)
			.contains(Permissions::ADMINISTRATOR);
		if let Some(nick) = body.get("nick") {
			member.nick = nick.as_str().filter(|nick| !nick.is_empty()).map(str::to_owned);
		}
		if body["avatar"].is_string() {
			member.avatar = Some(format!("{:032x}", store.next_snowflake()));
		}
		if let Some(bio) = body.get("bio") {
			let bio = bio.as_str().unwrap_or_default().to_owned();
			store.guild_bios.insert((guild_id, user_id), bio);
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
			let until = serde_json::from_value::<Option<DateTime<Utc>>>(until.clone())
				.ok()
				.flatten();
			if let Some(until) = until {
				if target_admin {
					return Err(ApiError::missing_permissions());
				}
				if until - Utc::now() > Duration::days(MAX_TIMEOUT_DAYS) {
					return Err(ApiError::invalid_body("Timeout is longer than 28 days"));
				}
			}
			member.communication_disabled_until = until;
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
		let change = json!({ "guild_id": guild_id.to_string(), "user": user });
		self.hub.broadcast("GUILD_MEMBER_REMOVE", &change);
		if ban {
			self.hub.broadcast("GUILD_BAN_ADD", &change);
		}
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
		drop(store);
		self.hub.broadcast(
			"GUILD_BAN_REMOVE",
			&json!({ "guild_id": guild_id.to_string(), "user": self.ban_user(user_id) }),
		);
		Ok(())
	}

	fn ban_user(&self, user_id: u64) -> Value {
		self.fixtures.users.get(&user_id).map_or_else(
			|| json!({ "id": user_id.to_string(), "username": format!("banned_{user_id}"), "discriminator": "0", "avatar": null }),
			to_json,
		)
	}

	pub fn bans_json(
		&self,
		guild_id: u64,
		after: Option<u64>,
		limit: Option<usize>,
	) -> Result<Value, ApiError> {
		let store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let bans: Vec<Value> = store
			.bans
			.get(&guild_id)
			.into_iter()
			.flatten()
			.filter(|(user_id, _)| after.is_none_or(|after| **user_id > after))
			.take(limit.unwrap_or(BAN_PAGE).min(BAN_PAGE))
			.map(|(user_id, reason)| json!({ "user": self.ban_user(*user_id), "reason": reason }))
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
		let guild = self
			.fixtures
			.guilds
			.iter()
			.find(|guild| guild.origin == GuildOrigin::Spare && store.absent_guilds.contains(&guild.id))
			.ok_or_else(ApiError::max_guilds)?;
		store.absent_guilds.remove(&guild.id);
		if let Some(properties) = store.properties.get_mut(&guild.id) {
			properties.name = Some(name.to_owned());
		}
		let payload = guild_create(self, &store, guild);
		drop(store);
		self.hub.broadcast("GUILD_CREATE", &payload);
		Ok(payload)
	}

	pub fn fixture_invite_guild(&self, code: &str) -> Option<&GuildFixture> {
		let index = match code.strip_prefix("mock")? {
			"" => 0,
			digits => digits.parse::<usize>().ok()?,
		};
		self.fixtures
			.guilds
			.get(index)
			.filter(|guild| guild.origin != GuildOrigin::Spare)
	}

	pub fn accept_invite(&self, code: &str) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let created = store
			.invites
			.get(code)
			.and_then(|invite| parse_id(&invite["guild_id"]))
			.and_then(|guild_id| self.fixtures.guild(guild_id));
		let guild = created
			.or_else(|| self.fixture_invite_guild(code))
			.ok_or_else(ApiError::unknown_invite)?;
		let new_member = store.absent_guilds.remove(&guild.id);
		if new_member {
			store.member_edits.remove(&(guild.id, CURRENT_USER_ID));
			if let Some(removed) = store.removed_members.get_mut(&guild.id) {
				removed.remove(&CURRENT_USER_ID);
			}
			if let Some(uses) = store.invites.get_mut(code).map(|invite| &mut invite["uses"]) {
				*uses = json!(uses.as_i64().unwrap_or(0) + 1);
			}
		}
		let channel = guild
			.message_channel_ids
			.first()
			.and_then(|id| store.channels.get(id));
		let member = store.member_now(guild, CURRENT_USER_ID);
		let mut invite = self.invite_value(&store, guild, channel);
		invite["code"] = json!(code);
		invite["new_member"] = json!(new_member);
		invite["show_verification_form"] =
			json!(new_member && member.as_ref().is_some_and(|member| member.pending == Some(true)));
		let created = new_member.then(|| guild_create(self, &store, guild));
		drop(store);
		if let Some(payload) = created {
			self.hub.broadcast("GUILD_CREATE", &payload);
			if member.is_some() {
				self.hub.broadcast("GUILD_MEMBER_ADD", &to_json(&member));
			}
		}
		Ok(invite)
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
		store.absent_guilds.insert(guild_id);
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
			recipients.push(user.clone());
		}
		channel.recipients = LenientVec::from(recipients);
		let current_user = store.current_user.clone();
		drop(store);
		self.hub.broadcast(
			if add {
				"CHANNEL_RECIPIENT_ADD"
			} else {
				"CHANNEL_RECIPIENT_REMOVE"
			},
			&json!({ "channel_id": channel_id.to_string(), "user": to_json(&user) }),
		);
		let notice = Draft {
			kind: Some(if add {
				MessageType::RecipientAdd
			} else {
				MessageType::RecipientRemove
			}),
			content: format!("<@{user_id}>"),
			..Draft::default()
		};
		self.post_message(channel_id, &current_user, notice)?;
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
		let member = store.thread_member_json(thread_id);
		drop(store);
		let mut update = json!({
			"id": thread_id.to_string(),
			"guild_id": guild_id,
			"member_count": count.max(0),
		});
		if join {
			update["added_members"] = json!([member]);
		} else {
			update["removed_member_ids"] = json!([CURRENT_USER_ID.to_string()]);
		}
		self.hub.broadcast("THREAD_MEMBERS_UPDATE", &update);
		Ok(())
	}

	pub fn thread_member_added(&self, thread_id: u64, user_id: u64) -> Result<(), ApiError> {
		if !self.fixtures.users.contains_key(&user_id) {
			return Err(ApiError::unknown_user());
		}
		let mut store = self.store.lock();
		let thread = store
			.channels
			.get(&thread_id)
			.filter(|channel| is_thread(channel.kind))
			.ok_or_else(ApiError::wrong_channel_kind)?;
		let closed = thread.kind == ChannelType::PrivateThread
			&& thread
				.thread_metadata
				.as_ref()
				.is_some_and(|metadata| metadata.invitable == Some(false));
		let moderator = thread
			.guild_id
			.and_then(|guild_id| self.fixtures.guild(guild_id.0))
			.is_some_and(|guild| {
				guild
					.permissions_for(guild.current_member())
					.contains(Permissions::MANAGE_THREADS)
			});
		if closed && !moderator {
			return Err(ApiError::missing_permissions());
		}
		if !store
			.thread_invitees
			.entry(thread_id)
			.or_default()
			.insert(user_id)
		{
			return Ok(());
		}
		let thread = store
			.channels
			.get_mut(&thread_id)
			.ok_or_else(ApiError::wrong_channel_kind)?;
		let count = thread.member_count.unwrap_or(0) + 1;
		thread.member_count = Some(count);
		let guild_id = thread.guild_id.map(|id| id.to_string());
		drop(store);
		self.hub.broadcast(
			"THREAD_MEMBERS_UPDATE",
			&json!({
				"id": thread_id.to_string(),
				"guild_id": guild_id,
				"member_count": count,
				"added_members": [{
					"id": thread_id.to_string(),
					"user_id": user_id.to_string(),
					"join_timestamp": now_iso(),
					"flags": 0,
				}],
			}),
		);
		Ok(())
	}

	pub fn update_thread_settings(&self, thread_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let guild_id = store
			.channels
			.get(&thread_id)
			.filter(|channel| is_thread(channel.kind))
			.ok_or_else(ApiError::wrong_channel_kind)?
			.guild_id;
		if !store.joined_threads.contains(&thread_id) {
			return Err(ApiError::unknown_member());
		}
		let settings = store
			.thread_member_settings
			.entry(thread_id)
			.or_insert_with(|| json!({}));
		for key in ["flags", "muted", "mute_config"] {
			if let Some(value) = body.get(key) {
				settings[key] = value.clone();
			}
		}
		let mut member = store.thread_member_json(thread_id);
		member["guild_id"] = json!(guild_id.map(|id| id.to_string()));
		drop(store);
		self.hub.broadcast("THREAD_MEMBER_UPDATE", &member);
		Ok(member)
	}

	pub fn threads_json(&self, filter: impl Fn(&Store, &Channel) -> bool) -> Value {
		let store = self.store.lock();
		let mut threads: Vec<&Channel> = store
			.channels
			.values()
			.filter(|channel| is_thread(channel.kind) && filter(&store, channel))
			.collect();
		threads.sort_by_key(|channel| std::cmp::Reverse(channel.id));
		let members: Vec<Value> = threads
			.iter()
			.filter(|thread| store.joined_threads.contains(&thread.id.0))
			.map(|thread| store.thread_member_json(thread.id.0))
			.collect();
		json!({
			"threads": threads.iter().map(|thread| store.thread_json(thread)).collect::<Vec<_>>(),
			"members": members,
			"has_more": false,
		})
	}

	pub fn set_pin(&self, channel_id: u64, message_id: u64, pinned: bool) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let channel = store
			.channels
			.get_mut(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?;
		let guild_id = channel.guild_id;
		if pinned {
			channel.last_pin_timestamp = Some(Utc::now());
		}
		let last_pin_timestamp = channel.last_pin_timestamp;
		let message = store
			.message_mut(channel_id, message_id)
			.ok_or_else(ApiError::unknown_message)?;
		message["pinned"] = json!(pinned);
		let value = message.clone();
		if pinned {
			store.pinned_at.insert(message_id, Utc::now());
		} else {
			store.pinned_at.remove(&message_id);
		}
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
				"last_pin_timestamp": last_pin_timestamp,
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
		let results = poll_results(message)?;
		if finalize {
			results["is_finalized"] = json!(true);
			let value = message.clone();
			let gateway = self.gateway_message(&store, &value);
			drop(store);
			self.hub.broadcast("MESSAGE_UPDATE", &gateway);
			return Ok(value);
		}
		let counts = answer_counts(results)?;
		let previous: Vec<i64> = counts
			.iter()
			.filter(|count| count["me_voted"].as_bool() == Some(true))
			.filter_map(|count| count["id"].as_i64())
			.collect();
		let removed: Vec<i64> = previous
			.iter()
			.copied()
			.filter(|id| !answers.contains(id))
			.collect();
		let added: Vec<i64> = answers
			.iter()
			.copied()
			.filter(|id| !previous.contains(id))
			.collect();
		for &id in &removed {
			adjust_answer(counts, id, -1, true);
		}
		for &id in &added {
			adjust_answer(counts, id, 1, true);
		}
		let value = message.clone();
		let guild_id = store
			.channels
			.get(&channel_id)
			.and_then(|channel| channel.guild_id);
		drop(store);
		for id in removed {
			self.announce_vote(
				"MESSAGE_POLL_VOTE_REMOVE",
				CURRENT_USER_ID,
				channel_id,
				message_id,
				guild_id,
				id,
			);
		}
		for id in added {
			self.announce_vote(
				"MESSAGE_POLL_VOTE_ADD",
				CURRENT_USER_ID,
				channel_id,
				message_id,
				guild_id,
				id,
			);
		}
		Ok(value)
	}

	pub fn cast_poll_vote(
		&self,
		user_id: u64,
		channel_id: u64,
		message_id: u64,
		answer_id: i64,
	) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let message = store
			.message_mut(channel_id, message_id)
			.ok_or_else(ApiError::unknown_message)?;
		let results = poll_results(message)?;
		adjust_answer(answer_counts(results)?, answer_id, 1, false);
		let guild_id = store
			.channels
			.get(&channel_id)
			.and_then(|channel| channel.guild_id);
		drop(store);
		self.announce_vote(
			"MESSAGE_POLL_VOTE_ADD",
			user_id,
			channel_id,
			message_id,
			guild_id,
			answer_id,
		);
		Ok(())
	}

	pub fn open_poll(&self) -> Option<(u64, u64, Vec<i64>)> {
		let store = self.store.lock();
		store.messages.iter().find_map(|(channel_id, messages)| {
			messages.iter().find_map(|(message_id, message)| {
				let poll = &message["poll"];
				let live = poll["expiry"]
					.as_str()
					.and_then(|expiry| DateTime::parse_from_rfc3339(expiry).ok())
					.is_none_or(|expiry| expiry > Utc::now());
				let open =
					poll.is_object() && poll["results"]["is_finalized"].as_bool() != Some(true) && live;
				open.then(|| {
					let answers = poll["answers"]
						.as_array()
						.map(|answers| answers.iter().filter_map(|a| a["answer_id"].as_i64()).collect())
						.unwrap_or_default();
					(*channel_id, *message_id, answers)
				})
			})
		})
	}

	fn announce_vote(
		&self,
		event: &str,
		user_id: u64,
		channel_id: u64,
		message_id: u64,
		guild_id: Option<Snowflake>,
		answer_id: i64,
	) {
		self.hub.broadcast(
			event,
			&json!({
				"user_id": user_id.to_string(),
				"channel_id": channel_id.to_string(),
				"message_id": message_id.to_string(),
				"guild_id": guild_id.map(|id| id.to_string()),
				"answer_id": answer_id,
			}),
		);
	}

	pub fn reaction_users(
		&self,
		channel_id: u64,
		message_id: u64,
		emoji: &str,
		limit: usize,
		burst: bool,
	) -> Result<Value, ApiError> {
		let store = self.store.lock();
		let message = store
			.message(channel_id, message_id)
			.ok_or_else(ApiError::unknown_message)?;
		let name = emoji.rsplit_once(':').map_or(emoji, |(name, _)| name);
		let side = if burst { "burst" } else { "normal" };
		let count = message["reactions"]
			.as_array()
			.into_iter()
			.flatten()
			.find(|reaction| reaction["emoji"]["name"].as_str() == Some(name))
			.and_then(|reaction| reaction["count_details"][side].as_u64())
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
