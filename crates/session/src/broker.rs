use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use plugin_host::HostBroker;
use plugin_host::api::model::{EmojiInfo, EmojiRef, Lifecycle, MessageFull, StickerInfo};
use plugin_host::api::{
	ChannelSummary, GuildSummary, HostError, MemberSummary, MessageRef, MessageView, OutgoingMessage, User,
	VoiceInfo,
};
use plugin_host::convert::{message_full, message_view, user_view};
use protocol::{Snowflake, StatusKind, Sticker};
use state::{ChannelMessages, MemberSlot, SharedState, State};
use tokio::sync::mpsc;

use crate::command::{Command, ReplyTarget};
use crate::task::Internal;

pub trait BrokerSink: Send + Sync {
	fn clipboard_write(&self, plugin: &str, text: &str) -> bool;

	fn notify(&self, plugin: &str, title: &str, body: &str) -> bool;

	fn open_url(&self, plugin: &str, url: &str) -> bool;
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

pub(crate) struct SessionBroker {
	pub(crate) state: SharedState,
	pub(crate) voice: Arc<VoiceCache>,
	pub(crate) commands: mpsc::Sender<Command>,
	pub(crate) internal: mpsc::UnboundedSender<Internal>,
	pub(crate) sink: Arc<dyn BrokerSink>,
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

impl SessionBroker {
	fn queue(&self, command: Command) -> Result<(), HostError> {
		self.commands
			.try_send(command)
			.map_err(|_| HostError::Unavailable)
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
}
