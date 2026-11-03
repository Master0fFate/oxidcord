use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use protocol::{
	Attachment, AttachmentDescriptor, AttachmentFlags, ChannelType, CreateMessage, EditMessage, LenientVec,
	Message, MessageFlags, MessageReference, MessageReferenceKind, MessageType, Nonce, Permissions, Poll,
	PollAnswer, PollCreate, ReadState, ReadStateKind, RelationshipKind, Snowflake, StickerFormat,
	StickerItem, User,
};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::fixtures::previews;
use crate::forward::snapshot_of;
use crate::json::{author_id, parse_id, to_json};
use crate::ready::is_thread;
use crate::state::{Shared, Store};

const BURST_COLOR: &str = "#ff73fa";
const MAX_POLL_ANSWERS: usize = 10;

pub struct UploadedFile {
	pub filename: String,
	pub content_type: Option<String>,
	pub size: usize,
}

#[derive(Default)]
pub struct Draft {
	pub id: Option<u64>,
	pub kind: Option<MessageType>,
	pub content: String,
	pub nonce: Option<String>,
	pub enforce_nonce: bool,
	pub tts: bool,
	pub reference: Option<MessageReference>,
	pub flags: MessageFlags,
	pub sticker_ids: Vec<Snowflake>,
	pub descriptors: Vec<AttachmentDescriptor>,
	pub files: Vec<UploadedFile>,
	pub poll: Option<PollCreate>,
	pub skip_echo: bool,
}

impl Draft {
	pub fn text(content: impl Into<String>) -> Self {
		Self {
			content: content.into(),
			..Self::default()
		}
	}

	pub fn is_blank(&self) -> bool {
		self.content.is_empty() && self.files.is_empty() && self.sticker_ids.is_empty() && self.poll.is_none()
	}

	pub fn from_create(create: CreateMessage, files: Vec<UploadedFile>) -> Self {
		Self {
			id: None,
			kind: None,
			content: create.content.unwrap_or_default(),
			nonce: create.nonce,
			enforce_nonce: create.enforce_nonce.unwrap_or(false),
			tts: create.tts.unwrap_or(false),
			reference: create.message_reference,
			flags: create.flags.unwrap_or_default(),
			sticker_ids: create.sticker_ids,
			descriptors: create.attachments,
			files,
			poll: create.poll,
			skip_echo: false,
		}
	}
}

pub fn now_iso() -> String {
	Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn scan_mentions(content: &str) -> (Vec<u64>, Vec<u64>) {
	let mut users = Vec::new();
	let mut roles = Vec::new();
	let mut rest = content;
	while let Some(start) = rest.find("<@") {
		let tail = &rest[start + 2..];
		let (is_role, tail) = match tail.strip_prefix('&') {
			Some(stripped) => (true, stripped),
			None => (false, tail.strip_prefix('!').unwrap_or(tail)),
		};
		let digits = tail.chars().take_while(|c| c.is_ascii_digit()).count();
		if digits > 0
			&& tail[digits..].starts_with('>')
			&& let Ok(id) = tail[..digits].parse::<u64>()
		{
			if is_role {
				roles.push(id);
			} else {
				users.push(id);
			}
		}
		rest = &rest[start + 2..];
	}
	(users, roles)
}

pub(crate) fn sanitized_filename(name: &str) -> String {
	name.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
				c
			} else {
				'_'
			}
		})
		.collect()
}

impl Shared {
	pub fn partial_member(&self, guild_id: u64, user_id: u64) -> Value {
		let Some(member) = self
			.fixtures
			.guild(guild_id)
			.and_then(|guild| guild.member(user_id))
		else {
			return Value::Null;
		};
		let mut value = to_json(member);
		if let Some(fields) = value.as_object_mut() {
			fields.remove("user");
		}
		value
	}

	pub fn gateway_message(&self, store: &Store, message: &Value) -> Value {
		let mut value = message.clone();
		let guild_id = parse_id(&message["channel_id"])
			.and_then(|channel_id| store.channels.get(&channel_id))
			.and_then(|channel| channel.guild_id);
		if let Some(guild_id) = guild_id {
			value["guild_id"] = json!(guild_id.to_string());
			value["member"] = self.partial_member(guild_id.0, author_id(message));
		}
		value
	}

	fn guild_of(store: &Store, channel_id: u64) -> Result<Option<Snowflake>, ApiError> {
		store
			.channels
			.get(&channel_id)
			.map(|channel| channel.guild_id)
			.ok_or_else(ApiError::unknown_channel)
	}

	fn blocked_conversation(store: &Store, channel_id: u64) -> bool {
		store.channels.get(&channel_id).is_some_and(|channel| {
			channel.kind == ChannelType::Dm
				&& channel.recipient_ids.iter().any(|id| {
					store
						.relationships
						.get(&id.0)
						.is_some_and(|relationship| relationship.kind == RelationshipKind::Blocked)
				})
		})
	}

	fn reactions_denied(&self, store: &Store, channel_id: u64) -> bool {
		let Some(channel) = store.channels.get(&channel_id) else {
			return false;
		};
		let Some(guild) = channel.guild_id.and_then(|id| self.fixtures.guild(id.0)) else {
			return false;
		};
		let me = guild.current_member();
		!guild.permissions_for(me).contains(Permissions::ADMINISTRATOR)
			&& channel.permission_overwrites.iter().any(|overwrite| {
				overwrite.deny.contains(Permissions::ADD_REACTIONS) && me.roles.contains(&overwrite.id)
			})
	}

	pub fn enforce_thread_unlocked(&self, channel_id: u64) -> Result<(), ApiError> {
		let store = self.store.lock();
		let Some(channel) = store.channels.get(&channel_id) else {
			return Ok(());
		};
		let locked = is_thread(channel.kind)
			&& channel
				.thread_metadata
				.as_ref()
				.is_some_and(|metadata| metadata.locked);
		let moderator = channel
			.guild_id
			.and_then(|guild_id| self.fixtures.guild(guild_id.0))
			.is_some_and(|guild| {
				guild
					.permissions_for(guild.current_member())
					.contains(Permissions::MANAGE_THREADS)
			});
		if locked && !moderator {
			return Err(ApiError::thread_locked());
		}
		Ok(())
	}

	pub fn enforce_slowmode(&self, channel_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let Some((guild_id, seconds)) = store.channels.get(&channel_id).and_then(|channel| {
			let seconds = channel.rate_limit_per_user.filter(|seconds| *seconds > 0)?;
			Some((channel.guild_id?.0, seconds))
		}) else {
			return Ok(());
		};
		let Some(guild) = self.fixtures.guild(guild_id) else {
			return Ok(());
		};
		if guild
			.permissions_for(guild.current_member())
			.contains(Permissions::BYPASS_SLOWMODE)
		{
			return Ok(());
		}
		let interval = Duration::from_secs(seconds.unsigned_abs());
		let now = Instant::now();
		if let Some(last) = store.slowmode.get(&channel_id) {
			let elapsed = now.saturating_duration_since(*last);
			if elapsed < interval {
				return Err(ApiError::slowmode((interval - elapsed).as_secs_f64()));
			}
		}
		store.slowmode.insert(channel_id, now);
		Ok(())
	}

	pub fn post_message(&self, channel_id: u64, author: &User, draft: Draft) -> Result<Value, ApiError> {
		let forwarding = draft
			.reference
			.as_ref()
			.is_some_and(|reference| reference.kind == Some(MessageReferenceKind::Forward));
		if draft.is_blank() && !forwarding && draft.kind.is_none() {
			return Err(ApiError::empty_message());
		}
		let mut store = self.store.lock();
		let guild_id = Self::guild_of(&store, channel_id)?;
		if author.id.0 == CURRENT_USER_ID && Self::blocked_conversation(&store, channel_id) {
			return Err(ApiError::cannot_message_user());
		}
		let dedup_key = draft
			.nonce
			.clone()
			.filter(|_| draft.enforce_nonce && author.id.0 == CURRENT_USER_ID)
			.map(|nonce| (channel_id, nonce));
		if let Some(existing) = dedup_key
			.as_ref()
			.and_then(|key| store.nonces.get(key))
			.and_then(|id| store.message(channel_id, *id))
		{
			return Ok(existing.clone());
		}
		let id = draft.id.unwrap_or_else(|| store.next_snowflake());
		let (user_ids, mention_roles) = scan_mentions(&draft.content);
		let mentions: Vec<User> = user_ids
			.iter()
			.filter_map(|user_id| self.fixtures.users.get(user_id).cloned())
			.collect();
		let mention_everyone = draft.content.contains("@everyone") || draft.content.contains("@here");
		let poll = draft.poll.map(poll_from).transpose()?;
		let mut attachments = Vec::with_capacity(draft.files.len());
		for (index, file) in draft.files.iter().enumerate() {
			let attachment_id = store.next_snowflake();
			attachments.push(uploaded_attachment(
				channel_id,
				attachment_id,
				index,
				file,
				&draft.descriptors,
			));
		}
		let mut message = Message {
			id: Snowflake(id),
			channel_id: Snowflake(channel_id),
			author: author.clone(),
			content: draft.content,
			timestamp: Some(Utc::now()),
			tts: draft.tts,
			mention_everyone,
			mentions: LenientVec::from(mentions),
			mention_roles: mention_roles.into_iter().map(Snowflake).collect(),
			attachments: LenientVec::from(attachments),
			poll,
			nonce: draft.nonce.map(Nonce),
			flags: draft.flags,
			kind: draft.kind.unwrap_or_default(),
			sticker_items: LenientVec::from(
				draft
					.sticker_ids
					.iter()
					.map(|sticker_id| StickerItem {
						id: *sticker_id,
						name: "Wave".to_owned(),
						format_type: Some(StickerFormat::Png),
					})
					.collect::<Vec<_>>(),
			),
			..Message::default()
		};
		if let Some(reference) = draft.reference
			&& let Some(target) = reference.message_id
		{
			let source_channel = reference.channel_id.map_or(channel_id, Snowflake::get);
			let found = store
				.message(source_channel, target.0)
				.cloned()
				.and_then(|mut value| {
					value["referenced_message"] = Value::Null;
					serde_json::from_value::<Message>(value).ok()
				});
			match (found, reference.fail_if_not_exists) {
				(Some(source), _) => {
					if reference.kind == Some(MessageReferenceKind::Forward) {
						message.message_snapshots = LenientVec::from(vec![snapshot_of(source)]);
						message.flags |= MessageFlags::HAS_SNAPSHOT;
					} else {
						if message.kind == MessageType::Default {
							message.kind = MessageType::Reply;
						}
						message.referenced_message = Some(Box::new(source));
					}
					message.message_reference = Some(MessageReference {
						channel_id: Some(Snowflake(source_channel)),
						guild_id: guild_id.or(reference.guild_id),
						..reference
					});
				}
				(None, Some(false)) => {}
				(None, _) => return Err(ApiError::unknown_message()),
			}
		}
		let value = to_json(&message);
		store
			.messages
			.entry(channel_id)
			.or_default()
			.insert(id, value.clone());
		if let Some(key) = dedup_key {
			store.nonces.insert(key, id);
		}
		store.refresh_last_message(channel_id);
		let from_me = author.id.0 == CURRENT_USER_ID;
		let pings_me =
			message.mention_everyone || message.mentions.iter().any(|user| user.id.0 == CURRENT_USER_ID);
		if from_me {
			let state = store
				.read_states
				.entry(channel_id)
				.or_insert_with(|| blank_read_state(channel_id));
			state.last_message_id = Some(Snowflake(id));
			state.mention_count = Some(0);
		} else if pings_me && let Some(state) = store.read_states.get_mut(&channel_id) {
			state.mention_count = Some(state.mention_count.unwrap_or(0) + 1);
		}
		let gateway = self.gateway_message(&store, &value);
		drop(store);
		if !draft.skip_echo {
			self.announce_message(guild_id, &gateway);
		}
		self.unfurl(channel_id, id, guild_id, &message);
		Ok(value)
	}

	fn announce_message(&self, guild_id: Option<Snowflake>, message: &Value) {
		let large =
			guild_id.filter(|guild_id| self.fixtures.guild(guild_id.0).is_some_and(|guild| guild.large));
		match large {
			Some(guild_id) => {
				let updated = json!({
					"id": message["channel_id"],
					"last_message_id": message["id"],
				});
				let passive = json!({
					"guild_id": guild_id.to_string(),
					"updated_channels": [updated],
					"updated_voice_states": [],
					"removed_voice_states": [],
					"updated_members": [],
				});
				let unread = json!({
					"guild_id": guild_id.to_string(),
					"channel_unread_updates": [updated],
				});
				self.hub
					.broadcast_unless_passive(guild_id.0, "MESSAGE_CREATE", message, &passive, &unread);
			}
			None => self.hub.broadcast("MESSAGE_CREATE", message),
		}
	}

	fn unfurl(&self, channel_id: u64, id: u64, guild_id: Option<Snowflake>, message: &Message) {
		if message.flags.contains(MessageFlags::SUPPRESS_EMBEDS) {
			return;
		}
		let embeds = previews::embeds_for(&message.content);
		if embeds.is_empty() {
			return;
		}
		let embeds = to_json(&embeds);
		if let Some(stored) = self.store.lock().message_mut(channel_id, id) {
			stored["embeds"] = embeds.clone();
		}
		let mut update =
			json!({ "id": id.to_string(), "channel_id": channel_id.to_string(), "embeds": embeds });
		if let Some(guild_id) = guild_id {
			update["guild_id"] = json!(guild_id.to_string());
		}
		self.hub.broadcast("MESSAGE_UPDATE", &update);
	}

	pub fn edit_message(&self, channel_id: u64, id: u64, edit: EditMessage) -> Result<Value, ApiError> {
		self.edit_as(channel_id, id, CURRENT_USER_ID, edit)
	}

	pub fn edit_as(
		&self,
		channel_id: u64,
		id: u64,
		author: u64,
		edit: EditMessage,
	) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		let guild_id = Self::guild_of(&store, channel_id)?;
		let can_moderate = author == CURRENT_USER_ID
			&& guild_id
				.and_then(|guild_id| self.fixtures.guild(guild_id.0))
				.is_some_and(|guild| {
					guild
						.permissions_for(guild.current_member())
						.contains(Permissions::MANAGE_MESSAGES)
				});
		let message = store
			.message_mut(channel_id, id)
			.ok_or_else(ApiError::unknown_message)?;
		let moderating = edit.content.is_none() && edit.attachments.is_none() && edit.flags.is_some();
		if author_id(message) != author {
			if !moderating {
				return Err(ApiError::not_author());
			}
			let current: MessageFlags = serde_json::from_value(message["flags"].clone()).unwrap_or_default();
			let changed = edit.flags.map_or(MessageFlags::empty(), |flags| current ^ flags);
			if !can_moderate || !changed.difference(MessageFlags::SUPPRESS_EMBEDS).is_empty() {
				return Err(ApiError::missing_permissions());
			}
		}
		if let Some(content) = edit.content {
			message["content"] = Value::String(content);
		}
		if let Some(flags) = edit.flags {
			message["flags"] = json!(flags.bits());
		}
		if let (Some(keep), Some(attachments)) = (edit.attachments, message["attachments"].as_array_mut()) {
			attachments.retain(|attachment| {
				parse_id(&attachment["id"])
					.is_some_and(|attachment_id| keep.iter().any(|kept| kept.id.0 == attachment_id))
			});
		}
		if !moderating {
			message["edited_timestamp"] = Value::String(now_iso());
		}
		let value = message.clone();
		let gateway = self.gateway_message(&store, &value);
		drop(store);
		self.hub.broadcast("MESSAGE_UPDATE", &gateway);
		Ok(value)
	}

	pub fn delete_message(&self, channel_id: u64, id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let guild_id = Self::guild_of(&store, channel_id)?;
		store
			.messages
			.get_mut(&channel_id)
			.and_then(|messages| messages.remove(&id))
			.ok_or_else(ApiError::unknown_message)?;
		store.refresh_last_message(channel_id);
		drop(store);
		let mut data = json!({ "id": id.to_string(), "channel_id": channel_id.to_string() });
		if let Some(guild_id) = guild_id {
			data["guild_id"] = json!(guild_id.to_string());
		}
		self.hub.broadcast("MESSAGE_DELETE", &data);
		Ok(())
	}

	pub fn set_reaction(
		&self,
		channel_id: u64,
		id: u64,
		emoji: &str,
		add: bool,
		burst: bool,
	) -> Result<(), ApiError> {
		let (name, emoji_id) = split_emoji(emoji);
		let mut store = self.store.lock();
		let guild_id = Self::guild_of(&store, channel_id)?;
		if add && self.reactions_denied(&store, channel_id) {
			return Err(ApiError::missing_permissions());
		}
		let reactions = Self::reactions_of(&mut store, channel_id, id)?;
		let own_key = if burst { "me_burst" } else { "me" };
		let position = reactions
			.iter()
			.position(|reaction| reaction_matches(reaction, name, emoji_id));
		match (position, add) {
			(Some(index), true) => {
				if reactions[index][own_key].as_bool() != Some(true) {
					reactions[index][own_key] = json!(true);
					adjust_count(&mut reactions[index], burst, 1);
				}
			}
			(None, true) => {
				let mut reaction = blank_reaction(name, emoji_id);
				reaction[own_key] = json!(true);
				adjust_count(&mut reaction, burst, 1);
				reactions.push(reaction);
			}
			(Some(index), false) => {
				if reactions[index][own_key].as_bool() == Some(true) {
					reactions[index][own_key] = json!(false);
					adjust_count(&mut reactions[index], burst, -1);
				}
				if reactions[index]["count"].as_i64().unwrap_or(0) <= 0 {
					reactions.remove(index);
				}
			}
			(None, false) => return Ok(()),
		}
		let mut data = json!({
			"user_id": CURRENT_USER_ID.to_string(),
			"channel_id": channel_id.to_string(),
			"message_id": id.to_string(),
			"emoji": { "id": emoji_id, "name": name, "animated": false },
			"burst": burst,
			"type": i64::from(burst),
		});
		if burst && add {
			data["burst_colors"] = json!([BURST_COLOR]);
		}
		if let Some(guild_id) = guild_id {
			data["guild_id"] = json!(guild_id.to_string());
			if add {
				data["member"] = self.partial_member(guild_id.0, CURRENT_USER_ID);
			}
		}
		drop(store);
		self.hub.broadcast(
			if add {
				"MESSAGE_REACTION_ADD"
			} else {
				"MESSAGE_REACTION_REMOVE"
			},
			&data,
		);
		Ok(())
	}

	pub fn reaction_storm(
		&self,
		channel_id: u64,
		id: u64,
		emoji: &str,
		user_ids: &[u64],
	) -> Result<(), ApiError> {
		let (name, emoji_id) = split_emoji(emoji);
		let mut store = self.store.lock();
		let guild_id = Self::guild_of(&store, channel_id)?;
		let reactions = Self::reactions_of(&mut store, channel_id, id)?;
		let index = reactions
			.iter()
			.position(|reaction| reaction_matches(reaction, name, emoji_id))
			.unwrap_or_else(|| {
				reactions.push(blank_reaction(name, emoji_id));
				reactions.len() - 1
			});
		for _ in user_ids {
			adjust_count(&mut reactions[index], false, 1);
		}
		let mut data = json!({
			"channel_id": channel_id.to_string(),
			"message_id": id.to_string(),
			"reactions": [{
				"emoji": { "id": emoji_id, "name": name, "animated": false },
				"users": user_ids.iter().map(u64::to_string).collect::<Vec<_>>(),
			}],
		});
		if let Some(guild_id) = guild_id {
			data["guild_id"] = json!(guild_id.to_string());
		}
		let singles: Vec<Value> = user_ids
			.iter()
			.map(|user_id| {
				let mut single = json!({
					"user_id": user_id.to_string(),
					"channel_id": channel_id.to_string(),
					"message_id": id.to_string(),
					"emoji": { "id": emoji_id, "name": name, "animated": false },
					"burst": false,
					"type": 0,
				});
				if let Some(guild_id) = guild_id {
					single["guild_id"] = json!(guild_id.to_string());
				}
				single
			})
			.collect();
		drop(store);
		self.hub
			.broadcast_batched("MESSAGE_REACTION_ADD_MANY", &data, &singles);
		Ok(())
	}

	fn reactions_of(store: &mut Store, channel_id: u64, id: u64) -> Result<&mut Vec<Value>, ApiError> {
		store
			.message_mut(channel_id, id)
			.and_then(Value::as_object_mut)
			.map(|fields| fields.entry("reactions").or_insert_with(|| json!([])))
			.and_then(Value::as_array_mut)
			.ok_or_else(ApiError::unknown_message)
	}

	pub fn typing(&self, channel_id: u64, user_id: u64) -> Result<(), ApiError> {
		let guild_id = Self::guild_of(&self.store.lock(), channel_id)?;
		let mut data = json!({
			"channel_id": channel_id.to_string(),
			"user_id": user_id.to_string(),
			"timestamp": Utc::now().timestamp(),
		});
		if let Some(guild_id) = guild_id {
			data["guild_id"] = json!(guild_id.to_string());
			data["member"] = self.partial_member(guild_id.0, user_id);
			data["member"]["user"] = self.fixtures.users.get(&user_id).map_or(Value::Null, to_json);
		}
		self.hub.broadcast("TYPING_START", &data);
		Ok(())
	}

	pub fn ack(&self, channel_id: u64, id: u64, mention_count: Option<i64>) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		Self::guild_of(&store, channel_id)?;
		let count = mention_count.unwrap_or(0);
		let state = store
			.read_states
			.entry(channel_id)
			.or_insert_with(|| blank_read_state(channel_id));
		state.last_message_id = Some(Snowflake(id));
		state.mention_count = Some(count);
		drop(store);
		let data = json!({
			"channel_id": channel_id.to_string(),
			"message_id": id.to_string(),
			"version": 1,
			"mention_count": count,
			"ack_type": 0,
			"flags": 0,
		});
		self.hub.broadcast("MESSAGE_ACK", &data);
		Ok(())
	}

	pub fn ack_pins(&self, channel_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let last_pin_timestamp = store
			.channels
			.get(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?
			.last_pin_timestamp;
		store
			.read_states
			.entry(channel_id)
			.or_insert_with(|| blank_read_state(channel_id))
			.last_pin_timestamp = last_pin_timestamp;
		drop(store);
		self.hub.broadcast(
			"CHANNEL_PINS_ACK",
			&json!({
				"channel_id": channel_id.to_string(),
				"timestamp": last_pin_timestamp,
				"version": 1,
			}),
		);
		Ok(())
	}
}

fn poll_from(create: PollCreate) -> Result<Poll, ApiError> {
	let question = create
		.question
		.text
		.as_deref()
		.is_some_and(|text| !text.is_empty());
	if !question || create.answers.is_empty() || create.answers.len() > MAX_POLL_ANSWERS {
		return Err(ApiError::invalid_body("Invalid Form Body"));
	}
	let answers = create
		.answers
		.into_iter()
		.zip(1..)
		.map(|(answer, answer_id)| PollAnswer {
			answer_id: Some(answer_id),
			..answer
		})
		.collect::<Vec<_>>();
	Ok(Poll {
		question: create.question,
		answers: LenientVec::from(answers),
		expiry: Some(Utc::now() + chrono::Duration::hours(i64::from(create.duration))),
		allow_multiselect: create.allow_multiselect,
		layout_type: Some(create.layout_type),
		results: None,
	})
}

fn blank_read_state(channel_id: u64) -> ReadState {
	ReadState {
		id: Snowflake(channel_id),
		flags: Some(0),
		mention_count: Some(0),
		read_state_type: Some(ReadStateKind::Channel),
		..ReadState::default()
	}
}

fn reaction_matches(reaction: &Value, name: &str, emoji_id: Option<&str>) -> bool {
	let emoji = &reaction["emoji"];
	match emoji_id {
		Some(emoji_id) => emoji["id"].as_str() == Some(emoji_id),
		None => emoji["id"].is_null() && emoji["name"].as_str() == Some(name),
	}
}

fn split_emoji(emoji: &str) -> (&str, Option<&str>) {
	match emoji.rsplit_once(':') {
		Some((name, emoji_id)) if emoji_id.parse::<u64>().is_ok() => (name, Some(emoji_id)),
		_ => (emoji, None),
	}
}

fn blank_reaction(name: &str, emoji_id: Option<&str>) -> Value {
	json!({
		"count": 0,
		"count_details": { "burst": 0, "normal": 0 },
		"me": false,
		"me_burst": false,
		"emoji": { "id": emoji_id, "name": name, "animated": false },
		"burst_colors": [BURST_COLOR],
	})
}

fn adjust_count(reaction: &mut Value, burst: bool, delta: i64) {
	let side = if burst { "burst" } else { "normal" };
	let details = &mut reaction["count_details"];
	details[side] = json!(details[side].as_i64().unwrap_or(0) + delta);
	let total = details["burst"].as_i64().unwrap_or(0) + details["normal"].as_i64().unwrap_or(0);
	reaction["count"] = json!(total);
}

fn uploaded_attachment(
	channel_id: u64,
	attachment_id: u64,
	index: usize,
	file: &UploadedFile,
	descriptors: &[AttachmentDescriptor],
) -> Attachment {
	let filename = sanitized_filename(&file.filename);
	let descriptor = descriptors
		.iter()
		.find(|descriptor| descriptor.id.0 == index as u64);
	let spoiler = descriptor.is_some_and(|descriptor| descriptor.is_spoiler == Some(true));
	Attachment {
		id: Snowflake(attachment_id),
		url: Some(format!(
			"https://cdn.discordapp.com/attachments/{channel_id}/{attachment_id}/{filename}"
		)),
		proxy_url: Some(format!(
			"https://media.discordapp.net/attachments/{channel_id}/{attachment_id}/{filename}"
		)),
		filename,
		title: descriptor.and_then(|descriptor| descriptor.title.clone()),
		description: descriptor.and_then(|descriptor| descriptor.description.clone()),
		content_type: file.content_type.clone(),
		size: Some(file.size as i64),
		flags: if spoiler {
			AttachmentFlags::IS_SPOILER
		} else {
			AttachmentFlags::empty()
		},
		..Attachment::default()
	}
}
