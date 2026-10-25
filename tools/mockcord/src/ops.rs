use chrono::{SecondsFormat, Utc};
use protocol::{
	Attachment, AttachmentDescriptor, CreateMessage, EditMessage, LenientVec, Message, MessageFlags,
	MessageReference, MessageReferenceKind, MessageType, Nonce, ReadState, ReadStateKind, Snowflake,
	StickerFormat, StickerItem, User,
};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::forward::snapshot_of;
use crate::json::{author_id, parse_id, to_json};
use crate::state::{Shared, Store};

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
	pub tts: bool,
	pub reference: Option<MessageReference>,
	pub flags: MessageFlags,
	pub sticker_ids: Vec<Snowflake>,
	pub descriptors: Vec<AttachmentDescriptor>,
	pub files: Vec<UploadedFile>,
}

impl Draft {
	pub fn text(content: impl Into<String>) -> Self {
		Self {
			content: content.into(),
			..Self::default()
		}
	}

	pub fn from_create(create: CreateMessage, files: Vec<UploadedFile>) -> Self {
		Self {
			id: None,
			kind: None,
			content: create.content.unwrap_or_default(),
			nonce: create.nonce,
			tts: create.tts.unwrap_or(false),
			reference: create.message_reference,
			flags: create.flags.unwrap_or_default(),
			sticker_ids: create.sticker_ids,
			descriptors: create.attachments,
			files,
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

fn sanitized_filename(name: &str) -> String {
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

	pub fn post_message(&self, channel_id: u64, author: &User, draft: Draft) -> Result<Value, ApiError> {
		let forwarding = draft
			.reference
			.as_ref()
			.is_some_and(|reference| reference.kind == Some(MessageReferenceKind::Forward));
		if draft.content.is_empty() && draft.files.is_empty() && draft.sticker_ids.is_empty() && !forwarding {
			return Err(ApiError::empty_message());
		}
		let mut store = self.store.lock();
		let guild_id = Self::guild_of(&store, channel_id)?;
		let id = draft.id.unwrap_or_else(|| store.next_snowflake());
		let (user_ids, mention_roles) = scan_mentions(&draft.content);
		let mentions: Vec<User> = user_ids
			.iter()
			.filter_map(|user_id| self.fixtures.users.get(user_id).cloned())
			.collect();
		let mention_everyone = draft.content.contains("@everyone") || draft.content.contains("@here");
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
		self.hub.broadcast("MESSAGE_CREATE", &gateway);
		Ok(value)
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
		Self::guild_of(&store, channel_id)?;
		let message = store
			.message_mut(channel_id, id)
			.ok_or_else(ApiError::unknown_message)?;
		if author_id(message) != author {
			return Err(ApiError::not_author());
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
		message["edited_timestamp"] = Value::String(now_iso());
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

	pub fn set_reaction(&self, channel_id: u64, id: u64, emoji: &str, add: bool) -> Result<(), ApiError> {
		let (name, emoji_id) = match emoji.rsplit_once(':') {
			Some((name, emoji_id)) if emoji_id.parse::<u64>().is_ok() => (name, Some(emoji_id)),
			_ => (emoji, None),
		};
		let mut store = self.store.lock();
		let guild_id = Self::guild_of(&store, channel_id)?;
		let message = store
			.message_mut(channel_id, id)
			.ok_or_else(ApiError::unknown_message)?;
		let reactions = message
			.as_object_mut()
			.map(|fields| fields.entry("reactions").or_insert_with(|| json!([])))
			.and_then(Value::as_array_mut)
			.ok_or_else(ApiError::unknown_message)?;
		let position = reactions
			.iter()
			.position(|reaction| reaction_matches(reaction, name, emoji_id));
		match (position, add) {
			(Some(index), true) => {
				if reactions[index]["me"].as_bool() != Some(true) {
					reactions[index]["me"] = json!(true);
					adjust_count(&mut reactions[index], 1);
				}
			}
			(None, true) => reactions.push(json!({
				"count": 1,
				"count_details": { "burst": 0, "normal": 1 },
				"me": true,
				"me_burst": false,
				"emoji": { "id": emoji_id, "name": name, "animated": false },
				"burst_colors": [],
			})),
			(Some(index), false) => {
				if reactions[index]["me"].as_bool() == Some(true) {
					reactions[index]["me"] = json!(false);
					adjust_count(&mut reactions[index], -1);
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
			"burst": false,
			"type": 0,
		});
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

fn adjust_count(reaction: &mut Value, delta: i64) {
	let count = reaction["count"].as_i64().unwrap_or(0) + delta;
	reaction["count"] = json!(count);
	reaction["count_details"]["normal"] = json!(count);
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
		flags: Some(0),
		..Attachment::default()
	}
}
