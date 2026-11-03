use std::sync::Arc;

use protocol::rest::{
	AttachmentDescriptor, CloudUploadRequest, CloudUploadSlot, CloudUploadSlots, ComponentInteraction,
	CreateMessage, EditMessage, MessagesQuery,
};
use protocol::{
	BURST_REACTION, Channel, ChannelType, CommandIndex, LenientVec, Message, PinnedPage, Snowflake,
	ThreadMember, User,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::guild_admin::with_reason;
use crate::client::RestClient;
use crate::error::{RestError, Result};
use crate::request::{Request, UploadFile, UploadProgress, encode_component};

const CONSENT_ACCEPTED: i64 = 2;
const MESSAGE_REQUEST_BATCH: usize = 50;
const CLOUD_UPLOAD_MIN_BYTES: usize = 8 * 1024 * 1024;

fn descriptor(index: usize, file: &UploadFile, uploaded_filename: Option<String>) -> AttachmentDescriptor {
	AttachmentDescriptor {
		id: Snowflake(index as u64),
		filename: Some(file.name.clone()),
		description: file.description.clone(),
		uploaded_filename,
		is_spoiler: file.is_spoiler().then_some(true),
		..AttachmentDescriptor::default()
	}
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
	pub content: Option<String>,
	pub author_ids: Vec<Snowflake>,
	pub mentions: Vec<Snowflake>,
	pub has: Vec<String>,
	pub before: Option<Snowflake>,
	pub after: Option<Snowflake>,
	pub offset: Option<u32>,
	pub channel_ids: Vec<Snowflake>,
	pub pinned: Option<bool>,
	pub author_types: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchivedScope {
	Public,
	Private,
	JoinedPrivate,
}

impl ArchivedScope {
	fn path(self, channel_id: Snowflake) -> String {
		match self {
			Self::Public => format!("/channels/{channel_id}/threads/archived/public"),
			Self::Private => format!("/channels/{channel_id}/threads/archived/private"),
			Self::JoinedPrivate => format!("/channels/{channel_id}/users/@me/threads/archived/private"),
		}
	}
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThreadList {
	pub threads: LenientVec<Channel>,
	pub members: LenientVec<ThreadMember>,
	pub has_more: bool,
}

fn messages_pairs(query: &MessagesQuery) -> Vec<(&'static str, String)> {
	let mut pairs = Vec::new();
	if let Some(around) = query.around {
		pairs.push(("around", around.to_string()));
	}
	if let Some(before) = query.before {
		pairs.push(("before", before.to_string()));
	}
	if let Some(after) = query.after {
		pairs.push(("after", after.to_string()));
	}
	if let Some(limit) = query.limit {
		pairs.push(("limit", limit.to_string()));
	}
	pairs
}

fn search_pairs(query: SearchQuery, channel_id: Option<Snowflake>) -> Vec<(&'static str, String)> {
	let mut pairs = Vec::new();
	if let Some(content) = query.content {
		pairs.push(("content", content));
	}
	pairs.extend(
		query
			.author_ids
			.into_iter()
			.map(|id| ("author_id", id.to_string())),
	);
	pairs.extend(query.mentions.into_iter().map(|id| ("mentions", id.to_string())));
	pairs.extend(query.has.into_iter().map(|has| ("has", has)));
	if let Some(before) = query.before {
		pairs.push(("max_id", before.to_string()));
	}
	if let Some(after) = query.after {
		pairs.push(("min_id", after.to_string()));
	}
	if let Some(offset) = query.offset {
		pairs.push(("offset", offset.to_string()));
	}
	if let Some(pinned) = query.pinned {
		pairs.push(("pinned", pinned.to_string()));
	}
	pairs.extend(query.author_types.into_iter().map(|kind| ("author_type", kind)));
	pairs.extend(
		channel_id
			.into_iter()
			.chain(query.channel_ids)
			.map(|id| ("channel_id", id.to_string())),
	);
	pairs
}

fn reaction_path(channel_id: Snowflake, message_id: Snowflake, emoji: &str) -> String {
	format!(
		"/channels/{channel_id}/messages/{message_id}/reactions/{}",
		encode_component(emoji)
	)
}

impl RestClient {
	pub async fn channel(&self, id: Snowflake) -> Result<Channel> {
		self.send(Request::get(format!("/channels/{id}"))).await
	}

	pub async fn messages(&self, channel_id: Snowflake, query: MessagesQuery) -> Result<LenientVec<Message>> {
		let request = Request::get(format!("/channels/{channel_id}/messages")).query(&messages_pairs(&query));
		self.send(request).await
	}

	pub async fn message(&self, channel_id: Snowflake, id: Snowflake) -> Result<Message> {
		self.send(Request::get(format!("/channels/{channel_id}/messages/{id}")))
			.await
	}

	pub async fn create_message(&self, channel_id: Snowflake, body: CreateMessage) -> Result<Message> {
		let mut request = Request::post(format!("/channels/{channel_id}/messages")).json(&body);
		if body.nonce.is_some() {
			request = request.idempotent();
		}
		self.send(request).await
	}

	pub async fn create_message_with_files(
		&self,
		channel_id: Snowflake,
		mut body: CreateMessage,
		files: Vec<UploadFile>,
		progress: Option<UploadProgress>,
	) -> Result<Message> {
		let total: usize = files.iter().map(|file| file.bytes.len()).sum();
		if total > CLOUD_UPLOAD_MIN_BYTES {
			return self
				.create_message_from_cloud(channel_id, body, files, progress)
				.await;
		}
		if body.attachments.is_empty() {
			body.attachments = files
				.iter()
				.enumerate()
				.map(|(index, file)| descriptor(index, file, None))
				.collect();
		}
		let mut request = Request::post(format!("/channels/{channel_id}/messages")).multipart(files, &body);
		if let Some(progress) = progress {
			request = request.progress(progress);
		}
		if body.nonce.is_some() {
			request = request.idempotent();
		}
		self.send(request).await
	}

	async fn create_message_from_cloud(
		&self,
		channel_id: Snowflake,
		mut body: CreateMessage,
		files: Vec<UploadFile>,
		progress: Option<UploadProgress>,
	) -> Result<Message> {
		let slots = self.upload_slots(channel_id, &files).await?;
		let mut uploaded = 0u64;
		for (file, slot) in files.iter().zip(&slots) {
			let report = progress.clone().map(|report| -> UploadProgress {
				let before = uploaded;
				Arc::new(move |sent| report(before + sent))
			});
			self.put_upload(&slot.upload_url, file, report).await?;
			uploaded += file.bytes.len() as u64;
		}
		body.attachments = files
			.iter()
			.zip(&slots)
			.enumerate()
			.map(|(index, (file, slot))| descriptor(index, file, Some(slot.upload_filename.clone())))
			.collect();
		self.create_message(channel_id, body).await
	}

	async fn upload_slots(
		&self,
		channel_id: Snowflake,
		files: &[UploadFile],
	) -> Result<Vec<CloudUploadSlot>> {
		let requested: Vec<CloudUploadRequest> = files
			.iter()
			.enumerate()
			.map(|(index, file)| CloudUploadRequest {
				id: Snowflake(index as u64),
				filename: file.name.clone(),
				file_size: file.bytes.len() as u64,
			})
			.collect();
		let request =
			Request::post(format!("/channels/{channel_id}/attachments")).json(&json!({ "files": requested }));
		let slots: CloudUploadSlots = self.send(request).await?;
		let mut slots = slots.attachments.into_vec();
		slots.sort_by_key(|slot| slot.id);
		if slots.len() == files.len() {
			Ok(slots)
		} else {
			Err(RestError::InvalidRequest)
		}
	}

	pub async fn edit_message(
		&self,
		channel_id: Snowflake,
		id: Snowflake,
		body: EditMessage,
	) -> Result<Message> {
		let request = Request::patch(format!("/channels/{channel_id}/messages/{id}")).json(&body);
		self.send(request).await
	}

	pub async fn delete_message(&self, channel_id: Snowflake, id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/channels/{channel_id}/messages/{id}")))
			.await
	}

	pub async fn pin_message(&self, channel_id: Snowflake, id: Snowflake) -> Result<()> {
		self.send_empty(Request::put(format!("/channels/{channel_id}/messages/pins/{id}")))
			.await
	}

	pub async fn unpin_message(&self, channel_id: Snowflake, id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!(
			"/channels/{channel_id}/messages/pins/{id}"
		)))
		.await
	}

	pub async fn pins(&self, channel_id: Snowflake, before: Option<&str>, limit: u8) -> Result<PinnedPage> {
		let mut request = Request::get(format!("/channels/{channel_id}/messages/pins"));
		if let Some(before) = before {
			request = request.query(&[("before", before.to_owned())]);
		}
		self.send(request.query(&[("limit", limit.to_string())])).await
	}

	pub async fn ack_pins(&self, channel_id: Snowflake) -> Result<()> {
		self.send_empty(Request::post(format!("/channels/{channel_id}/pins/ack")))
			.await
	}

	pub async fn trigger_typing(&self, channel_id: Snowflake) -> Result<()> {
		self.send_empty(Request::post(format!("/channels/{channel_id}/typing")))
			.await
	}

	pub async fn ack_message(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		body: Value,
	) -> Result<Value> {
		let request = Request::post(format!("/channels/{channel_id}/messages/{message_id}/ack")).json(&body);
		self.send(request).await
	}

	pub async fn add_reaction(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: &str,
	) -> Result<()> {
		let path = format!("{}/@me", reaction_path(channel_id, message_id, emoji));
		self.send_empty(Request::put(path)).await
	}

	pub async fn add_burst_reaction(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: &str,
	) -> Result<()> {
		let path = format!("{}/@me", reaction_path(channel_id, message_id, emoji));
		self.send_empty(Request::put(path).query(&[("location", "Message"), ("type", "1")]))
			.await
	}

	pub async fn remove_own_reaction(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: &str,
		burst: bool,
	) -> Result<()> {
		let base = reaction_path(channel_id, message_id, emoji);
		let path = match burst {
			true => format!("{base}/{BURST_REACTION}/@me"),
			false => format!("{base}/@me"),
		};
		self.send_empty(Request::delete(path)).await
	}

	pub async fn reactions(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: &str,
		limit: u8,
		burst: bool,
	) -> Result<LenientVec<User>> {
		let mut request =
			Request::get(reaction_path(channel_id, message_id, emoji)).query(&[("limit", limit.to_string())]);
		if burst {
			request = request.query(&[("type", BURST_REACTION.to_string())]);
		}
		self.send(request).await
	}

	pub async fn create_dm(&self, recipient_id: Snowflake) -> Result<Channel> {
		let request = Request::post("/users/@me/channels").json(&json!({ "recipients": [recipient_id] }));
		self.send(request).await
	}

	pub async fn create_group_dm(&self, recipient_ids: &[Snowflake]) -> Result<Channel> {
		let request = Request::post("/users/@me/channels").json(&json!({ "recipients": recipient_ids }));
		self.send(request).await
	}

	pub async fn ring_call(&self, channel_id: Snowflake, recipients: Option<Vec<Snowflake>>) -> Result<()> {
		let request = Request::post(format!("/channels/{channel_id}/call/ring"))
			.json(&json!({ "recipients": recipients }));
		self.send_empty(request).await
	}

	pub async fn stop_ringing(
		&self,
		channel_id: Snowflake,
		recipients: Option<Vec<Snowflake>>,
	) -> Result<()> {
		let request = Request::post(format!("/channels/{channel_id}/call/stop-ringing"))
			.json(&json!({ "recipients": recipients }));
		self.send_empty(request).await
	}

	pub async fn search_messages(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		query: SearchQuery,
	) -> Result<Value> {
		let (path, channel_filter) = match (guild_id, channel_id) {
			(Some(guild_id), channel_id) => (format!("/guilds/{guild_id}/messages/search"), channel_id),
			(None, Some(channel_id)) => (format!("/channels/{channel_id}/messages/search"), None),
			(None, None) => return Err(RestError::InvalidRequest),
		};
		self.send_indexed(Request::get(path).query(&search_pairs(query, channel_filter)))
			.await
	}

	pub async fn threads_active(&self, guild_id: Snowflake) -> Result<ThreadList> {
		self.send(Request::get(format!("/guilds/{guild_id}/threads/active")))
			.await
	}

	pub async fn threads_archived(
		&self,
		channel_id: Snowflake,
		scope: ArchivedScope,
		before: Option<&str>,
		limit: Option<u8>,
	) -> Result<ThreadList> {
		let mut request = Request::get(scope.path(channel_id));
		if let Some(before) = before {
			request = request.query(&[("before", before.to_owned())]);
		}
		if let Some(limit) = limit {
			request = request.query(&[("limit", limit.to_string())]);
		}
		self.send(request).await
	}

	pub async fn start_thread(
		&self,
		channel_id: Snowflake,
		message_id: Option<Snowflake>,
		name: &str,
		auto_archive: u32,
		kind: ChannelType,
		invitable: Option<bool>,
	) -> Result<Channel> {
		let request = match message_id {
			Some(message_id) => {
				Request::post(format!("/channels/{channel_id}/messages/{message_id}/threads"))
					.json(&json!({ "name": name, "auto_archive_duration": auto_archive }))
			}
			None => {
				let mut body = json!({ "name": name, "auto_archive_duration": auto_archive, "type": kind });
				if let Some(invitable) = invitable {
					body["invitable"] = json!(invitable);
				}
				Request::post(format!("/channels/{channel_id}/threads")).json(&body)
			}
		};
		self.send(request).await
	}

	pub async fn join_thread(&self, channel_id: Snowflake) -> Result<()> {
		self.send_empty(Request::put(format!("/channels/{channel_id}/thread-members/@me")))
			.await
	}

	pub async fn add_thread_member(&self, channel_id: Snowflake, user_id: Snowflake) -> Result<()> {
		self.send_empty(Request::put(format!(
			"/channels/{channel_id}/thread-members/{user_id}"
		)))
		.await
	}

	pub async fn update_thread_settings(
		&self,
		channel_id: Snowflake,
		settings: Value,
	) -> Result<ThreadMember> {
		self.send(
			Request::patch(format!("/channels/{channel_id}/thread-members/@me/settings")).json(&settings),
		)
		.await
	}

	pub async fn leave_thread(&self, channel_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!(
			"/channels/{channel_id}/thread-members/@me"
		)))
		.await
	}

	pub async fn create_invite(&self, channel_id: Snowflake, options: Value) -> Result<Value> {
		self.send(Request::post(format!("/channels/{channel_id}/invites")).json(&options))
			.await
	}

	pub async fn invite(&self, code: &str) -> Result<Value> {
		let request =
			Request::get(format!("/invites/{}", encode_component(code))).query(&[("with_counts", "true")]);
		self.send(request).await
	}

	pub async fn accept_invite(&self, code: &str) -> Result<Value> {
		let request = Request::post(format!("/invites/{}", encode_component(code))).json(&json!({}));
		self.send(request).await
	}

	pub async fn delete_invite(&self, code: &str, reason: Option<&str>) -> Result<()> {
		let request = Request::delete(format!("/invites/{}", encode_component(code)));
		self.send_empty(with_reason(request, reason)).await
	}

	pub async fn delete_channel(&self, id: Snowflake) -> Result<Channel> {
		self.send(Request::delete(format!("/channels/{id}"))).await
	}

	pub async fn modify_channel(&self, id: Snowflake, changes: Value) -> Result<Channel> {
		self.send(Request::patch(format!("/channels/{id}")).json(&changes))
			.await
	}

	pub async fn set_voice_status(&self, id: Snowflake, status: Option<&str>) -> Result<()> {
		let request = Request::put(format!("/channels/{id}/voice-status")).json(&json!({ "status": status }));
		self.send_empty(request).await
	}

	pub async fn voice_regions(&self) -> Result<Value> {
		self.send(Request::get("/voice/regions")).await
	}
}

impl RestClient {
	pub async fn create_forum_post(
		&self,
		channel_id: Snowflake,
		body: Value,
		files: Vec<UploadFile>,
	) -> Result<Channel> {
		let path = format!("/channels/{channel_id}/threads");
		let request = if files.is_empty() {
			Request::post(path).json(&body)
		} else {
			Request::post(path).multipart(files, &body)
		};
		self.send(request).await
	}

	pub async fn vote_poll(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		answer_ids: &[i64],
	) -> Result<()> {
		let request = Request::put(format!("/channels/{channel_id}/polls/{message_id}/answers/@me"))
			.json(&json!({ "answer_ids": answer_ids }));
		self.send_empty(request).await
	}

	pub async fn save_message(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		due_at: Option<String>,
	) -> Result<()> {
		let request = Request::put(format!("/users/@me/saved-messages/{channel_id}/{message_id}"))
			.json(&json!({ "due_at": due_at }));
		self.send_empty(request).await
	}

	pub async fn crosspost_message(&self, channel_id: Snowflake, message_id: Snowflake) -> Result<()> {
		self.send(Request::post(format!(
			"/channels/{channel_id}/messages/{message_id}/crosspost"
		)))
		.await
		.map(|_: Message| ())
	}

	pub async fn guild_command_index(&self, guild_id: Snowflake) -> Result<CommandIndex> {
		self.send(Request::get(format!(
			"/guilds/{guild_id}/application-command-index"
		)))
		.await
	}

	pub async fn channel_command_index(&self, channel_id: Snowflake) -> Result<CommandIndex> {
		self.send(Request::get(format!(
			"/channels/{channel_id}/application-command-index"
		)))
		.await
	}

	pub async fn user_command_index(&self) -> Result<CommandIndex> {
		self.send(Request::get("/users/@me/application-command-index"))
			.await
	}

	pub async fn create_interaction(&self, body: &Value) -> Result<()> {
		self.send_empty(Request::post("/interactions").json(body)).await
	}

	pub async fn component_interaction(&self, body: &ComponentInteraction) -> Result<()> {
		self.send_empty(Request::post("/interactions").json(body)).await
	}

	pub async fn end_poll(&self, channel_id: Snowflake, message_id: Snowflake) -> Result<Message> {
		self.send(Request::post(format!(
			"/channels/{channel_id}/polls/{message_id}/expire"
		)))
		.await
	}

	pub async fn add_group_recipient(&self, channel_id: Snowflake, user_id: Snowflake) -> Result<()> {
		self.send_empty(Request::put(format!(
			"/channels/{channel_id}/recipients/{user_id}"
		)))
		.await
	}

	pub async fn remove_group_recipient(&self, channel_id: Snowflake, user_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!(
			"/channels/{channel_id}/recipients/{user_id}"
		)))
		.await
	}

	pub async fn accept_message_request(&self, channel_id: Snowflake) -> Result<()> {
		let request = Request::put(format!("/channels/{channel_id}/recipients/@me"))
			.json(&json!({ "consent_status": CONSENT_ACCEPTED }));
		self.send_empty(request).await
	}

	pub async fn reject_message_request(&self, channel_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/channels/{channel_id}/recipients/@me")))
			.await
	}

	pub async fn reject_message_requests(&self, channel_ids: &[Snowflake]) -> Result<()> {
		for batch in channel_ids.chunks(MESSAGE_REQUEST_BATCH) {
			let request =
				Request::put("/channels/recipients/@me/batch-reject").json(&json!({ "channel_ids": batch }));
			self.send_empty(request).await?;
		}
		Ok(())
	}

	pub async fn set_channel_overwrite(
		&self,
		channel_id: Snowflake,
		target_id: Snowflake,
		body: Value,
	) -> Result<()> {
		let request = Request::put(format!("/channels/{channel_id}/permissions/{target_id}")).json(&body);
		self.send_empty(request).await
	}

	pub async fn delete_channel_overwrite(&self, channel_id: Snowflake, target_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!(
			"/channels/{channel_id}/permissions/{target_id}"
		)))
		.await
	}

	pub async fn bulk_delete_messages(&self, channel_id: Snowflake, ids: &[Snowflake]) -> Result<()> {
		let request = Request::post(format!("/channels/{channel_id}/messages/bulk-delete"))
			.json(&json!({ "messages": ids }));
		self.send_empty(request).await
	}
}
