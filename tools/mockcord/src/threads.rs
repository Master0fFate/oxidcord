use chrono::Utc;
use protocol::{
	Channel, ChannelType, MessageFlags, MessageReference, MessageType, Snowflake, ThreadMetadata,
};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::json::to_json;
use crate::ops::Draft;
use crate::state::Shared;

pub struct ThreadRequest {
	pub name: String,
	pub auto_archive_duration: i64,
	pub kind: ChannelType,
	pub applied_tags: Vec<Snowflake>,
	pub starter: Option<Draft>,
}

impl Shared {
	pub fn create_thread(
		&self,
		parent_id: u64,
		from_message: Option<u64>,
		request: ThreadRequest,
	) -> Result<Value, ApiError> {
		let me = &self.fixtures.current_user;
		let (guild_id, parent_kind, thread_id) = {
			let mut store = self.store.lock();
			let parent = store
				.channels
				.get(&parent_id)
				.ok_or_else(ApiError::unknown_channel)?;
			let Some(guild_id) = parent.guild_id else {
				return Err(ApiError::wrong_channel_kind());
			};
			let parent_kind = parent.kind;
			if let Some(message_id) = from_message
				&& store.message(parent_id, message_id).is_none()
			{
				return Err(ApiError::unknown_message());
			}
			let thread_id = from_message.unwrap_or_else(|| store.next_snowflake());
			if store.channels.contains_key(&thread_id) {
				return Err(ApiError::invalid_body(
					"A thread has already been created for this message",
				));
			}
			(guild_id, parent_kind, thread_id)
		};
		let is_forum = matches!(parent_kind, ChannelType::GuildForum | ChannelType::GuildMedia);
		let kind = if is_forum || from_message.is_some() {
			if parent_kind == ChannelType::GuildAnnouncement {
				ChannelType::AnnouncementThread
			} else {
				ChannelType::PublicThread
			}
		} else {
			request.kind
		};
		let now = Utc::now();
		let thread = Channel {
			id: Snowflake(thread_id),
			kind,
			guild_id: Some(guild_id),
			parent_id: Some(Snowflake(parent_id)),
			name: Some(request.name),
			owner_id: Some(me.id),
			rate_limit_per_user: Some(0),
			message_count: Some(0),
			member_count: Some(1),
			total_message_sent: Some(0),
			applied_tags: request.applied_tags,
			thread_metadata: Some(ThreadMetadata {
				archived: false,
				auto_archive_duration: Some(request.auto_archive_duration),
				archive_timestamp: Some(now),
				locked: false,
				invitable: Some(true),
				create_timestamp: Some(now),
			}),
			..Channel::default()
		};
		{
			let mut store = self.store.lock();
			store.channels.insert(thread_id, thread);
			store.joined_threads.insert(thread_id);
		}

		let starter = match request.starter {
			Some(mut draft) if is_forum => {
				draft.id = Some(thread_id);
				Some(self.post_message(thread_id, me, draft)?)
			}
			_ => None,
		};
		if from_message.is_none() && !is_forum {
			let name = self.store.lock().channels[&thread_id]
				.name
				.clone()
				.unwrap_or_default();
			let notice = Draft {
				kind: Some(MessageType::ThreadCreated),
				content: name,
				reference: Some(MessageReference {
					channel_id: Some(Snowflake(thread_id)),
					..MessageReference::default()
				}),
				..Draft::default()
			};
			self.post_message(parent_id, me, notice)?;
		}
		if let Some(message_id) = from_message {
			self.mark_has_thread(parent_id, message_id, thread_id);
		}

		let mut store = self.store.lock();
		let created = store.channels[&thread_id].clone();
		let mut thread_json = store.thread_json(&created);
		thread_json["newly_created"] = json!(true);
		store.messages.entry(thread_id).or_default();
		drop(store);
		self.hub.broadcast("THREAD_CREATE", &thread_json);
		if let Some(starter) = starter {
			thread_json["message"] = starter;
		}
		thread_json["member"] = json!({
			"id": thread_id.to_string(),
			"user_id": CURRENT_USER_ID.to_string(),
			"join_timestamp": now.to_rfc3339(),
			"flags": 0,
		});
		Ok(thread_json)
	}

	fn mark_has_thread(&self, channel_id: u64, message_id: u64, thread_id: u64) {
		let mut store = self.store.lock();
		let thread = to_json(&store.channels[&thread_id]);
		let Some(message) = store.message_mut(channel_id, message_id) else {
			return;
		};
		let flags = message["flags"].as_u64().unwrap_or(0) | MessageFlags::HAS_THREAD.bits();
		message["flags"] = json!(flags);
		message["thread"] = thread;
		let value = message.clone();
		let gateway = self.gateway_message(&store, &value);
		drop(store);
		self.hub.broadcast("MESSAGE_UPDATE", &gateway);
	}
}
