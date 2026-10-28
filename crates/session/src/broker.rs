use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use parking_lot::Mutex;
use plugin_host::api::model::{
	AuthSession, ChannelDetail, ClientInfo, EmojiInfo, EmojiRef, Folder, GuildDetail, Lifecycle, MemberInfo,
	MessageFull, MessageMeta, PluginSummary, Presence, ReadState, Relationship, RoleInfo, StickerInfo,
	UserProfile, VoiceDetail,
};
use plugin_host::api::{
	ChannelSummary, GuildSummary, HostError, MemberSummary, MessageRef, MessageView, OutgoingMessage, User,
	VoiceInfo,
};
use plugin_host::convert::{message_full, message_view, user_view};
use plugin_host::{HostBroker, PluginManager};
use protocol::{Message, Snowflake, StatusKind, Sticker};
use sha2::{Digest, Sha256};
use state::{ChannelMessages, DM_PERMISSIONS, MemberSlot, SharedState, State};
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::command::{Command, PluginLoad, Reply, ReplyTarget};
use crate::loads::{PluginLoads, await_reply};
use crate::marks::ClientMarks;
use crate::task::Internal;
use crate::views;

const MAX_IMPLICIT: usize = 1_000;
const MAX_UNREAD_CHANNELS: usize = 2_000;

pub trait BrokerSink: Send + Sync {
	fn clipboard_write(&self, plugin: &str, text: &str) -> bool;

	fn notify(&self, plugin: &str, title: &str, body: &str) -> bool;

	fn open_url(&self, plugin: &str, url: &str) -> bool;

	fn idle_seconds(&self) -> Option<u64> {
		None
	}

	fn speak(&self, _plugin: &str, _text: &str, _rate: f32, _volume: f32) -> bool {
		false
	}
}

pub struct NoopSink;

impl BrokerSink for NoopSink {
	fn clipboard_write(&self, _plugin: &str, _text: &str) -> bool {
		false
	}

	fn notify(&self, _plugin: &str, _title: &str, _body: &str) -> bool {
		false
	}

	fn open_url(&self, _plugin: &str, _url: &str) -> bool {
		false
	}
}

#[derive(Default)]
pub(crate) struct VoiceCache {
	revision: AtomicU64,
	cached: Mutex<Option<(u64, Option<VoiceInfo>)>>,
}

impl VoiceCache {
	pub(crate) fn invalidate(&self) {
		self.revision.fetch_add(1, Ordering::AcqRel);
	}

	pub(crate) fn get(&self, state: &SharedState) -> Option<VoiceInfo> {
		let revision = self.revision.load(Ordering::Acquire);
		if let Some((cached, info)) = &*self.cached.lock()
			&& *cached == revision
		{
			return info.clone();
		}
		let info = voice_info(&state.read());
		*self.cached.lock() = Some((revision, info.clone()));
		info
	}
}

type ImplicitUsers = ((usize, usize, usize), Arc<[Snowflake]>);

#[derive(Default)]
pub(crate) struct BrokerCaches {
	implicit: Mutex<Option<ImplicitUsers>>,
	sessions_requested: AtomicBool,
	plugins: OnceLock<Weak<PluginManager>>,
}

pub(crate) struct SessionBroker {
	pub(crate) state: SharedState,
	pub(crate) voice: Arc<VoiceCache>,
	pub(crate) commands: mpsc::Sender<Command>,
	pub(crate) internal: mpsc::UnboundedSender<Internal>,
	pub(crate) sink: Arc<dyn BrokerSink>,
	pub(crate) handle: Handle,
	pub(crate) marks: Arc<ClientMarks>,
	pub(crate) loads: Arc<PluginLoads>,
	pub(crate) locale: String,
	pub(crate) caches: BrokerCaches,
}

pub(crate) fn view_of(
	message: &protocol::Message,
	channel_guild: impl FnOnce() -> Option<Snowflake>,
) -> MessageView {
	let mut view = message_view(message);
	view.guild_id = message.guild_id.or_else(channel_guild).map(Snowflake::get);
	view
}

pub(crate) fn full_of(
	state: &State,
	cache: &ChannelMessages,
	message: &protocol::Message,
	lifecycle: Lifecycle,
) -> MessageFull {
	let channel_id = message.channel_id;
	let view = view_of(message, || state.channel_guild(channel_id));
	let mine = state.current_user_id() == Some(message.author.id);
	let lifecycle = match lifecycle {
		Lifecycle::Live if cache.is_deleted(message.id) => Lifecycle::Deleted(0),
		other => other,
	};
	message_full(message, view, mine, lifecycle)
}

fn sticker_info(sticker: &Sticker) -> StickerInfo {
	StickerInfo {
		id: sticker.id.get(),
		name: sticker.name.clone(),
		format: sticker
			.format_type
			.map_or(0, |format| u8::try_from(i64::from(format)).unwrap_or(0)),
		guild_id: sticker.guild_id.map(Snowflake::get),
	}
}

pub(crate) fn voice_info(state: &State) -> Option<VoiceInfo> {
	let session = state.voice()?;
	let mut participants: Vec<u64> = match session.guild_id {
		Some(guild_id) => state
			.guild(guild_id)
			.map(|guild| {
				guild
					.voice_states
					.values()
					.filter(|voice| voice.channel_id == Some(session.channel_id))
					.map(|voice| voice.user_id.get())
					.collect()
			})
			.unwrap_or_default(),
		None => state
			.call(session.channel_id)
			.map(|call| call.participants.iter().map(|id| id.get()).collect())
			.unwrap_or_default(),
	};
	participants.sort_unstable();
	let mut speaking: Vec<u64> = session.speaking.iter().map(|id| id.get()).collect();
	speaking.sort_unstable();
	Some(VoiceInfo {
		guild_id: session.guild_id.map(Snowflake::get),
		channel_id: session.channel_id.get(),
		self_mute: session.self_mute,
		self_deaf: session.self_deaf,
		participants,
		speaking,
		e2ee: session.stats.e2ee,
	})
}

fn channel_summary(row: &state::ChannelRow, guild_id: Option<u64>) -> ChannelSummary {
	ChannelSummary {
		id: row.id.get(),
		guild_id,
		name: row.name.clone(),
		kind: u32::try_from(i64::from(row.kind)).unwrap_or(u32::MAX),
		parent_id: row.parent_id.map(Snowflake::get),
		position: u32::try_from(row.position).unwrap_or(0),
	}
}

fn delivered(accepted: bool) -> Result<(), HostError> {
	if accepted {
		Ok(())
	} else {
		Err(HostError::Unavailable)
	}
}

pub(crate) fn fetched_full(state: &State, message: &Message) -> MessageFull {
	let view = view_of(message, || state.channel_guild(message.channel_id));
	let mine = state.current_user_id() == Some(message.author.id);
	message_full(message, view, mine, Lifecycle::Live)
}

pub(crate) fn reaction_key(emoji: &EmojiRef) -> String {
	match emoji.id {
		Some(id) => format!("{}:{id}", emoji.name),
		None => emoji.name.clone(),
	}
}

fn is_private(state: &State, channel_id: Snowflake) -> bool {
	state.channel(channel_id).is_some() && state.channel_guild(channel_id).is_none()
}

fn current_session(sessions: &[state::AuthSession], session_id: &str) -> Option<usize> {
	let hashed = (!session_id.is_empty())
		.then(|| {
			let digest = Sha256::digest(session_id.as_bytes());
			[
				hex::encode(digest),
				STANDARD.encode(digest),
				URL_SAFE_NO_PAD.encode(digest),
			]
		})
		.and_then(|hashes| {
			sessions
				.iter()
				.position(|session| hashes.contains(&session.id_hash))
		});
	hashed.or_else(|| {
		sessions
			.iter()
			.enumerate()
			.max_by_key(|(_, session)| session.last_used)
			.map(|(index, _)| index)
	})
}

impl SessionBroker {
	fn queue(&self, command: Command) -> Result<(), HostError> {
		self.commands
			.try_send(command)
			.map_err(|_| HostError::Unavailable)
	}

	pub(crate) fn attach_plugins(&self, manager: &Arc<PluginManager>) {
		let _ = self.caches.plugins.set(Arc::downgrade(manager));
	}

	fn request<T>(
		&self,
		load: impl FnOnce(Reply<crate::command::LoadResult<T>>) -> PluginLoad,
	) -> crate::command::LoadResult<T> {
		let (reply, receiver) = Reply::channel();
		self.queue(Command::PluginLoad(load(reply)))?;
		await_reply(&self.handle, receiver)?
	}

	fn media(&self, url: String) -> Result<Vec<u8>, HostError> {
		self.loads.media.run(url.clone(), || {
			self.request(|reply| PluginLoad::Media { url, reply })
		})
	}

	fn find_message(
		&self,
		state: &State,
		channel_id: Snowflake,
		message_id: Snowflake,
	) -> Option<Arc<Message>> {
		state
			.messages(channel_id)
			.and_then(|cache| cache.by_id.get(&message_id).cloned())
			.or_else(|| self.loads.fetched_message(channel_id, message_id))
	}

	fn implicit_users(&self, state: &State) -> Arc<[Snowflake]> {
		let fingerprint = state.people_fingerprint();
		let mut cache = self.caches.implicit.lock();
		if let Some((known, users)) = &*cache
			&& *known == fingerprint
		{
			return Arc::clone(users);
		}
		let users: Arc<[Snowflake]> = state.implicit_relationships().into();
		*cache = Some((fingerprint, Arc::clone(&users)));
		users
	}
}

impl HostBroker for SessionBroker {
	fn send_message(&self, plugin: &str, msg: OutgoingMessage) -> Result<u64, HostError> {
		self.queue(Command::SendMessage {
			channel_id: Snowflake(msg.channel_id),
			content: msg.content,
			reply_to: msg.reply_to.map(|id| ReplyTarget {
				message: Snowflake(id),
				mention: true,
			}),
			files: Vec::new(),
			by_plugin: Some(plugin.to_owned()),
		})?;
		Ok(0)
	}

	fn add_reaction(&self, _plugin: &str, message: MessageRef, emoji: String) -> Result<(), HostError> {
		self.queue(Command::AddReaction {
			channel_id: Snowflake(message.channel_id),
			message_id: Snowflake(message.id),
			emoji,
		})
	}

	fn open_channel(&self, _plugin: &str, guild_id: Option<u64>, channel_id: u64) -> Result<(), HostError> {
		self.queue(Command::SelectChannel {
			guild_id: guild_id.map(Snowflake),
			channel_id: Snowflake(channel_id),
		})
	}

	fn clipboard_write(&self, plugin: &str, text: String) -> Result<(), HostError> {
		delivered(self.sink.clipboard_write(plugin, &text))
	}

	fn notify(&self, plugin: &str, title: String, body: String) -> Result<(), HostError> {
		if self.state.read().own_status() == StatusKind::Dnd {
			return Ok(());
		}
		delivered(self.sink.notify(plugin, &title, &body))
	}

	fn open_url(&self, plugin: &str, url: String) -> Result<(), HostError> {
		delivered(self.sink.open_url(plugin, &url))
	}

	fn current_user(&self, _plugin: &str) -> Result<User, HostError> {
		self.state
			.read()
			.current_user()
			.map(user_view)
			.ok_or(HostError::Unavailable)
	}

	fn current_channel(&self, _plugin: &str) -> Result<Option<(Option<u64>, u64)>, HostError> {
		Ok(self
			.state
			.read()
			.selected()
			.map(|selected| (selected.guild_id.map(Snowflake::get), selected.channel_id.get())))
	}

	fn channel_messages(
		&self,
		_plugin: &str,
		channel_id: u64,
		limit: u32,
	) -> Result<Vec<MessageView>, HostError> {
		let channel_id = Snowflake(channel_id);
		let state = self.state.read();
		let cache = state.messages(channel_id).ok_or(HostError::Unavailable)?;
		let mut views: Vec<MessageView> = cache
			.ordered
			.iter()
			.rev()
			.filter(|message| !cache.is_deleted(message.id))
			.take(limit as usize)
			.map(|message| view_of(message, || state.channel_guild(channel_id)))
			.collect();
		views.reverse();
		Ok(views)
	}

	fn display_name(&self, _plugin: &str, guild_id: Option<u64>, user_id: u64) -> Result<String, HostError> {
		Ok(self
			.state
			.read()
			.display_name(guild_id.map(Snowflake), Snowflake(user_id)))
	}

	fn list_guilds(&self, _plugin: &str) -> Result<Vec<GuildSummary>, HostError> {
		let state = self.state.read();
		let me = state.current_user_id();
		Ok(state
			.guild_summaries()
			.into_iter()
			.map(|summary| {
				let guild = state.guild(summary.id);
				GuildSummary {
					id: summary.id.get(),
					name: summary.name,
					icon: summary.icon,
					owner: me.is_some() && guild.and_then(|guild| guild.owner_id) == me,
					member_count: guild
						.and_then(|guild| guild.member_count)
						.and_then(|count| u32::try_from(count).ok()),
				}
			})
			.collect())
	}

	fn list_channels(&self, _plugin: &str, guild_id: Option<u64>) -> Result<Vec<ChannelSummary>, HostError> {
		let state = self.state.read();
		let Some(guild_id) = guild_id else {
			return Ok(state
				.dm_rows()
				.into_iter()
				.map(|row| ChannelSummary {
					id: row.channel_id.get(),
					guild_id: None,
					name: row.name,
					kind: if row.recipients.len() > 1 { 3 } else { 1 },
					parent_id: None,
					position: 0,
				})
				.collect());
		};
		let guild = Snowflake(guild_id);
		if state.guild(guild).is_none() {
			return Err(HostError::Unavailable);
		}
		Ok(state
			.guild_channel_tree(guild)
			.iter()
			.flat_map(|node| node.category.iter().chain(&node.children))
			.map(|row| channel_summary(row, Some(guild_id)))
			.collect())
	}

	fn list_members(
		&self,
		_plugin: &str,
		guild_id: u64,
		limit: u32,
	) -> Result<Vec<MemberSummary>, HostError> {
		let state = self.state.read();
		let guild = state.guild(Snowflake(guild_id)).ok_or(HostError::Unavailable)?;
		Ok(guild
			.member_list
			.slots
			.iter()
			.filter_map(|slot| match slot {
				MemberSlot::Member(user_id) => Some(*user_id),
				MemberSlot::Group { .. } | MemberSlot::Empty => None,
			})
			.take(limit as usize)
			.map(|user_id| {
				let member = state.member_row(guild, user_id);
				MemberSummary {
					user_id: member.user_id.get(),
					display_name: member.display,
					roles: guild
						.members
						.get(&member.user_id)
						.map(|entry| entry.roles.iter().map(|role| role.get()).collect())
						.unwrap_or_default(),
					status: member.status.as_str().to_owned(),
				}
			})
			.collect())
	}

	fn voice_state(&self, _plugin: &str) -> Result<Option<VoiceInfo>, HostError> {
		Ok(self.voice.get(&self.state))
	}

	fn surfaces_changed(&self) {
		let _ = self.internal.send(Internal::SlotsChanged);
	}

	fn send_as_user(&self, plugin: &str, msg: OutgoingMessage) -> Result<(), HostError> {
		self.queue(Command::SendAsUser {
			plugin_id: plugin.to_owned(),
			channel_id: Snowflake(msg.channel_id),
			content: msg.content,
			reply_to: msg.reply_to.map(|id| ReplyTarget {
				message: Snowflake(id),
				mention: true,
			}),
		})
	}

	fn list_emojis(
		&self,
		_plugin: &str,
		channel_id: Option<u64>,
		limit: u32,
	) -> Result<Vec<EmojiInfo>, HostError> {
		let state = self.state.read();
		let channel = channel_id
			.map(Snowflake)
			.or_else(|| state.selected().map(|selected| selected.channel_id));
		let guild = channel.and_then(|channel| state.channel_guild(channel));
		Ok(state
			.catalog_emojis(guild)
			.take(limit as usize)
			.map(|emoji| EmojiInfo {
				emoji: EmojiRef {
					id: Some(emoji.id.get()),
					name: emoji.name,
					animated: emoji.animated,
				},
				guild_id: Some(emoji.guild_id.get()),
				usable: emoji.usable,
				available: emoji.available,
			})
			.collect())
	}

	fn list_stickers(&self, _plugin: &str, guild_id: Option<u64>) -> Result<Vec<StickerInfo>, HostError> {
		let state = self.state.read();
		let stickers: Vec<StickerInfo> = match guild_id {
			Some(guild_id) => state
				.guild_stickers(Snowflake(guild_id))
				.unwrap_or_default()
				.iter()
				.map(sticker_info)
				.collect(),
			None => state
				.sticker_packs()
				.unwrap_or_default()
				.iter()
				.flat_map(|pack| pack.stickers.iter())
				.map(sticker_info)
				.collect(),
		};
		Ok(stickers)
	}

	fn message_full(
		&self,
		_plugin: &str,
		channel_id: u64,
		message_id: u64,
	) -> Result<Option<MessageFull>, HostError> {
		let state = self.state.read();
		let cache = state
			.messages(Snowflake(channel_id))
			.ok_or(HostError::Unavailable)?;
		Ok(cache
			.by_id
			.get(&Snowflake(message_id))
			.map(|message| full_of(&state, cache, message, Lifecycle::Live)))
	}

	fn channel_messages_full(
		&self,
		_plugin: &str,
		channel_id: u64,
		limit: u32,
	) -> Result<Vec<MessageFull>, HostError> {
		let state = self.state.read();
		let cache = state
			.messages(Snowflake(channel_id))
			.ok_or(HostError::Unavailable)?;
		let mut messages: Vec<MessageFull> = cache
			.ordered
			.iter()
			.rev()
			.take(limit as usize)
			.map(|message| full_of(&state, cache, message, Lifecycle::Live))
			.collect();
		messages.reverse();
		Ok(messages)
	}

	fn relationships(&self, _plugin: &str) -> Result<Vec<Relationship>, HostError> {
		let state = self.state.read();
		let mut found: Vec<Relationship> = state
			.relationships()
			.filter_map(|relationship| views::relationship(&state, relationship))
			.collect();
		found.extend(
			self.implicit_users(&state)
				.iter()
				.take(MAX_IMPLICIT)
				.filter_map(|user_id| views::implicit(&state, *user_id)),
		);
		Ok(found)
	}

	fn presence(&self, _plugin: &str, user_id: u64) -> Result<Option<Presence>, HostError> {
		Ok(views::presence(&self.state.read(), Snowflake(user_id)))
	}

	fn idle_seconds(&self, _plugin: &str) -> Result<u32, HostError> {
		self.sink
			.idle_seconds()
			.map(|seconds| u32::try_from(seconds).unwrap_or(u32::MAX))
			.ok_or(HostError::Unavailable)
	}

	fn read_state(
		&self,
		_plugin: &str,
		channel_id: u64,
		private: bool,
	) -> Result<Option<ReadState>, HostError> {
		let state = self.state.read();
		let channel_id = Snowflake(channel_id);
		if !private && is_private(&state, channel_id) {
			return Ok(None);
		}
		Ok(state.channel_unread(channel_id).map(views::read_state))
	}

	fn unread_channels(
		&self,
		_plugin: &str,
		guild_id: Option<u64>,
		private: bool,
	) -> Result<Vec<ReadState>, HostError> {
		Ok(self
			.state
			.read()
			.unread_channels(guild_id.map(Snowflake), private)
			.into_iter()
			.take(MAX_UNREAD_CHANNELS)
			.map(views::read_state)
			.collect())
	}

	fn channel_muted(&self, _plugin: &str, channel_id: u64, private: bool) -> Result<bool, HostError> {
		let state = self.state.read();
		let channel_id = Snowflake(channel_id);
		if !private && is_private(&state, channel_id) {
			return Ok(false);
		}
		Ok(state
			.channel_unread(channel_id)
			.is_some_and(|unread| unread.muted))
	}

	fn roles(&self, _plugin: &str, guild_id: u64) -> Result<Vec<RoleInfo>, HostError> {
		views::roles(&self.state.read(), Snowflake(guild_id)).ok_or(HostError::Unavailable)
	}

	fn member_info(
		&self,
		_plugin: &str,
		guild_id: u64,
		user_id: u64,
	) -> Result<Option<MemberInfo>, HostError> {
		Ok(views::member_info(
			&self.state.read(),
			Snowflake(guild_id),
			Snowflake(user_id),
		))
	}

	fn channel_permissions(
		&self,
		_plugin: &str,
		channel_id: u64,
		user_id: Option<u64>,
		private: bool,
	) -> Result<u64, HostError> {
		let state = self.state.read();
		let channel_id = Snowflake(channel_id);
		match state.channel_guild(channel_id) {
			Some(guild_id) => {
				let user = user_id
					.map(Snowflake)
					.or_else(|| state.current_user_id())
					.ok_or(HostError::Unavailable)?;
				Ok(state.permissions_in(guild_id, channel_id, user).bits())
			}
			None if private && is_private(&state, channel_id) => Ok(DM_PERMISSIONS.bits()),
			None => Err(HostError::Unavailable),
		}
	}

	fn guild_detail(&self, _plugin: &str, guild_id: u64) -> Result<Option<GuildDetail>, HostError> {
		Ok(views::guild_detail(&self.state.read(), Snowflake(guild_id)))
	}

	fn channel_detail(
		&self,
		_plugin: &str,
		channel_id: u64,
		private: bool,
	) -> Result<Option<ChannelDetail>, HostError> {
		Ok(views::channel_detail(
			&self.state.read(),
			Snowflake(channel_id),
			private,
		))
	}

	fn folders(&self, _plugin: &str) -> Result<Vec<Folder>, HostError> {
		Ok(views::folders(&self.state.read()))
	}

	fn cached_profile(&self, _plugin: &str, user_id: u64) -> Option<UserProfile> {
		self.state
			.read()
			.profile(Snowflake(user_id))
			.map(views::user_profile)
	}

	fn load_profile(&self, _plugin: &str, user_id: u64) -> Result<Option<UserProfile>, HostError> {
		let user_id = Snowflake(user_id);
		self.loads.profiles.run(user_id, || {
			self.request(|reply| PluginLoad::Profile { user_id, reply })
		})
	}

	fn sessions(&self, _plugin: &str) -> Result<Vec<AuthSession>, HostError> {
		let state = self.state.read();
		let Some(found) = state.auth_sessions() else {
			drop(state);
			if !self.caches.sessions_requested.swap(true, Ordering::AcqRel) {
				self.queue(Command::LoadSessions)?;
			}
			return Err(HostError::Unavailable);
		};
		let current = current_session(found, &state.session().session_id);
		Ok(found
			.iter()
			.enumerate()
			.map(|(index, session)| AuthSession {
				id: session.id_hash.clone(),
				client_os: session.os.clone(),
				client_platform: session.platform.clone(),
				location: session.location.clone(),
				approx_last_used_ms: session.last_used.map_or(0, |time| time.timestamp_millis()),
				current: current == Some(index),
			})
			.collect())
	}

	fn client_info(&self, _plugin: &str) -> Result<ClientInfo, HostError> {
		Ok(ClientInfo {
			version: env!("CARGO_PKG_VERSION").to_owned(),
			platform: std::env::consts::OS.to_owned(),
			uptime_ms: i64::try_from(self.marks.uptime_ms()).unwrap_or(i64::MAX),
			startup: self.marks.startup(),
			locale: self.locale.clone(),
		})
	}

	fn voice_detail(&self, _plugin: &str, _private: bool) -> Result<Option<VoiceDetail>, HostError> {
		Ok(views::voice_detail(&self.state.read()))
	}

	fn cached_message(&self, _plugin: &str, channel_id: u64, message_id: u64) -> Option<MessageFull> {
		let state = self.state.read();
		let (channel_id, message_id) = (Snowflake(channel_id), Snowflake(message_id));
		if let Some(cache) = state.messages(channel_id)
			&& let Some(message) = cache.by_id.get(&message_id)
		{
			return Some(full_of(&state, cache, message, Lifecycle::Live));
		}
		let message = self.loads.fetched_message(channel_id, message_id)?;
		Some(fetched_full(&state, &message))
	}

	fn load_message(
		&self,
		_plugin: &str,
		channel_id: u64,
		message_id: u64,
		private: bool,
	) -> Result<Option<MessageFull>, HostError> {
		let (channel_id, message_id) = (Snowflake(channel_id), Snowflake(message_id));
		if !private && is_private(&self.state.read(), channel_id) {
			return Ok(None);
		}
		self.loads.messages.run((channel_id, message_id), || {
			self.request(|reply| PluginLoad::Message {
				channel_id,
				message_id,
				reply,
			})
		})
	}

	fn message_meta(
		&self,
		_plugin: &str,
		message: &MessageRef,
		private: bool,
	) -> Result<Option<MessageMeta>, HostError> {
		let state = self.state.read();
		let channel_id = Snowflake(message.channel_id);
		if !private && is_private(&state, channel_id) {
			return Ok(None);
		}
		let Some(found) = self.find_message(&state, channel_id, Snowflake(message.id)) else {
			return Ok(None);
		};
		let meta = views::message_meta(&state, state.messages(channel_id), &found);
		Ok(Some(meta).filter(|meta| meta.guild_id.is_some() || private))
	}

	fn cached_reaction_users(
		&self,
		_plugin: &str,
		message: &MessageRef,
		emoji: &EmojiRef,
		limit: u32,
		private: bool,
	) -> Option<Vec<User>> {
		let state = self.state.read();
		if !private && is_private(&state, Snowflake(message.channel_id)) {
			return Some(Vec::new());
		}
		state
			.reaction_users(Snowflake(message.id), &reaction_key(emoji))
			.map(|users| users.iter().take(limit as usize).map(user_view).collect())
	}

	fn load_reaction_users(
		&self,
		_plugin: &str,
		message: &MessageRef,
		emoji: &EmojiRef,
		limit: u32,
		private: bool,
	) -> Result<Vec<User>, HostError> {
		let (channel_id, message_id) = (Snowflake(message.channel_id), Snowflake(message.id));
		if !private && is_private(&self.state.read(), channel_id) {
			return Ok(Vec::new());
		}
		let emoji = reaction_key(emoji);
		let mut users = self
			.loads
			.reactions
			.run((channel_id, message_id, emoji.clone()), || {
				self.request(|reply| PluginLoad::ReactionUsers {
					channel_id,
					message_id,
					emoji,
					reply,
				})
			})?;
		users.truncate(limit as usize);
		Ok(users)
	}

	fn cached_user(&self, _plugin: &str, user_id: u64) -> Option<User> {
		self.state
			.read()
			.user(Snowflake(user_id))
			.map(|user| user_view(user))
	}

	fn load_user(&self, _plugin: &str, user_id: u64) -> Result<Option<User>, HostError> {
		let user_id = Snowflake(user_id);
		self.loads.users.run(user_id, || {
			self.request(|reply| PluginLoad::User { user_id, reply })
		})
	}

	fn attachment_bytes(
		&self,
		_plugin: &str,
		message: &MessageRef,
		attachment_id: u64,
		private: bool,
	) -> Result<Vec<u8>, HostError> {
		let url = {
			let state = self.state.read();
			let channel_id = Snowflake(message.channel_id);
			if !private && is_private(&state, channel_id) {
				return Err(HostError::Unavailable);
			}
			let found = self
				.find_message(&state, channel_id, Snowflake(message.id))
				.ok_or(HostError::Unavailable)?;
			found
				.attachments
				.iter()
				.find(|attachment| attachment.id.get() == attachment_id)
				.and_then(|attachment| attachment.url.clone())
				.ok_or(HostError::Unavailable)?
		};
		self.media(url)
	}

	fn avatar_bytes(&self, _plugin: &str, user_id: u64, size: u32) -> Result<Vec<u8>, HostError> {
		let url = {
			let state = self.state.read();
			let user = state.user(Snowflake(user_id));
			media::cdn::avatar_url(
				user_id,
				user.and_then(|user| user.avatar.as_deref()),
				user.and_then(|user| user.discriminator.as_deref()),
				size,
			)
		};
		self.media(url)
	}

	fn favorite_emojis(&self, _plugin: &str) -> Result<Vec<EmojiRef>, HostError> {
		Ok(Vec::new())
	}

	fn plugins(&self, _plugin: &str) -> Result<Vec<PluginSummary>, HostError> {
		self.caches
			.plugins
			.get()
			.and_then(Weak::upgrade)
			.map(|manager| manager.summaries())
			.ok_or(HostError::Unavailable)
	}

	fn composer_draft(&self, _plugin: &str, private: bool) -> Result<String, HostError> {
		let state = self.state.read();
		let Some(selected) = state.selected() else {
			return Ok(String::new());
		};
		if !private && selected.guild_id.is_none() {
			return Ok(String::new());
		}
		Ok(state.draft(selected.channel_id).unwrap_or_default().to_owned())
	}

	fn speak(&self, plugin: &str, text: &str, rate: f32, volume: f32) -> Result<(), HostError> {
		delivered(self.sink.speak(plugin, text, rate, volume))
	}
}
