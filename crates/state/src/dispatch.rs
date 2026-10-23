use std::sync::Arc;
use std::time::Instant;

use protocol::{
	Channel, DispatchEvent, GuildMemberListUpdate, GuildMembersChunk, GuildRoleChange, Member,
	MemberListGroup, Message, MessageAck, MessageUpdate, Presence, Snowflake, ThreadDelete, ThreadListSync,
	TypingStart, UnavailableGuild, UserGuildSettings, VoiceState,
};
use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;

use crate::changes::Changes;
use crate::guild::{is_thread, stripped_member};
use crate::member_list::MemberSlot;
use crate::messages::{add_reaction, apply_update, remove_emoji, remove_reaction};
use crate::state::{State, intern_user};

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawListItem {
	group: Option<MemberListGroup>,
	member: Option<Box<RawValue>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawListPresence {
	presence: Option<Presence>,
}

impl State {
	pub(crate) fn apply_dispatch(&mut self, event: &DispatchEvent) -> Changes {
		match event {
			DispatchEvent::Ready(ready) => self.apply_ready(ready),
			DispatchEvent::ReadySupplemental(supplemental) => self.apply_supplemental(supplemental),
			DispatchEvent::Resumed => self.set_connected(true),
			DispatchEvent::MessageCreate(message) => self.message_create(message),
			DispatchEvent::MessageUpdate(update) => self.message_update(update),
			DispatchEvent::MessageDelete(delete) => self.messages_delete(delete.channel_id, &[delete.id]),
			DispatchEvent::MessageDeleteBulk(delete) => self.messages_delete(delete.channel_id, &delete.ids),
			DispatchEvent::MessageReactionAdd(add) => {
				let mine = Some(add.user_id) == self.current_user_id();
				self.edit_message(add.channel_id, add.message_id, |message| {
					add_reaction(message, &add.emoji, mine)
				})
			}
			DispatchEvent::MessageReactionRemove(remove) => {
				let mine = Some(remove.user_id) == self.current_user_id();
				self.edit_message(remove.channel_id, remove.message_id, |message| {
					remove_reaction(message, &remove.emoji, mine)
				})
			}
			DispatchEvent::MessageReactionRemoveAll(remove) => {
				self.edit_message(remove.channel_id, remove.message_id, |message| {
					message.reactions.clear()
				})
			}
			DispatchEvent::MessageReactionRemoveEmoji(remove) => {
				self.edit_message(remove.channel_id, remove.message_id, |message| {
					remove_emoji(message, &remove.emoji)
				})
			}
			DispatchEvent::MessageAck(ack) => self.message_ack(ack),
			DispatchEvent::ChannelCreate(channel)
			| DispatchEvent::ChannelUpdate(channel)
			| DispatchEvent::ThreadCreate(channel)
			| DispatchEvent::ThreadUpdate(channel) => self.upsert_channel(channel),
			DispatchEvent::ChannelDelete(channel) => self.remove_channel(channel.id),
			DispatchEvent::ThreadDelete(delete) => self.thread_delete(delete),
			DispatchEvent::ThreadListSync(sync) => self.thread_list_sync(sync),
			DispatchEvent::ThreadMemberUpdate(member) => {
				let mut changes = Changes::default();
				self.set_thread_member(member, &mut changes);
				changes
			}
			DispatchEvent::ThreadMembersUpdate(update) => self.thread_members_update(update),
			DispatchEvent::StreamCreate(stream) | DispatchEvent::StreamUpdate(stream) => {
				self.stream_changed(stream)
			}
			DispatchEvent::StreamDelete(delete) => self.stream_deleted(delete),
			DispatchEvent::GuildCreate(guild) => {
				self.ingest_guild(guild);
				self.reorder_guilds();
				self.guild_loaded_changes(guild.id)
			}
			DispatchEvent::GuildUpdate(guild) => {
				if let Some(state) = self.guilds.get_mut(&guild.id) {
					state.set_info(guild);
					state.set_roles_and_emojis(guild);
				}
				self.guild_loaded_changes(guild.id)
			}
			DispatchEvent::GuildDelete(guild) => self.guild_delete(guild),
			DispatchEvent::GuildMemberAdd(member) | DispatchEvent::GuildMemberUpdate(member) => {
				self.upsert_guild_member(member)
			}
			DispatchEvent::GuildMemberRemove(remove) => {
				let mut changes = Changes::default();
				if let Some(guild) = self.guilds.get_mut(&remove.guild_id) {
					guild.members.remove(&remove.user.id);
					guild.member_list.remove_member(remove.user.id);
					changes.members.insert(remove.guild_id);
				}
				changes
			}
			DispatchEvent::GuildMembersChunk(chunk) => self.members_chunk(chunk),
			DispatchEvent::GuildMemberListUpdate(update) => self.member_list_update(update),
			DispatchEvent::GuildRoleCreate(change) | DispatchEvent::GuildRoleUpdate(change) => {
				self.role_changed(change)
			}
			DispatchEvent::GuildRoleDelete(delete) => {
				let mut changes = Changes::default();
				if let Some(guild) = self.guilds.get_mut(&delete.guild_id)
					&& guild.roles.shift_remove(&delete.role_id).is_some()
				{
					self.permissions_changed(delete.guild_id, &mut changes);
				}
				changes
			}
			DispatchEvent::GuildEmojisUpdate(update) => {
				let mut changes = Changes::default();
				if let Some(guild) = self.guilds.get_mut(&update.guild_id) {
					guild.emojis = update.emojis.to_vec();
					changes.loaded.insert(update.guild_id);
				}
				changes
			}
			DispatchEvent::GuildStickersUpdate(update) => self.guild_stickers_updated(update),
			DispatchEvent::PresenceUpdate(presence) => {
				let mut changes = Changes::default();
				if let Some(user_id) = self.apply_presence(presence) {
					changes.presence.insert(user_id);
				}
				changes
			}
			DispatchEvent::TypingStart(typing) => self.typing_start(typing),
			DispatchEvent::UserUpdate(user) => {
				intern_user(&mut self.users, user);
				let mut changes = Changes::default();
				if Some(user.id) == self.current_user_id() {
					self.current_user = Some((**user).clone());
					changes.current_user = true;
				}
				changes
			}
			DispatchEvent::UserSettingsUpdate(settings) => self.apply_user_settings(settings),
			DispatchEvent::UserSettingsProtoUpdate(update) => self.apply_settings_proto_update(update),
			DispatchEvent::UserGuildSettingsUpdate(settings) => self.guild_settings_update(settings),
			DispatchEvent::RelationshipAdd(relationship) => {
				self.store_relationship(relationship);
				Changes {
					relationships: true,
					..Changes::default()
				}
			}
			DispatchEvent::RelationshipRemove(remove) => Changes {
				relationships: self.relationships.shift_remove(&remove.id).is_some(),
				..Changes::default()
			},
			DispatchEvent::VoiceStateUpdate(voice) => self.voice_state_update(voice),
			DispatchEvent::CallCreate(payload) | DispatchEvent::CallUpdate(payload) => {
				self.call_changed(payload)
			}
			DispatchEvent::CallDelete(payload) => self.call_deleted(payload),
			_ => Changes::default(),
		}
	}

	fn set_connected(&mut self, connected: bool) -> Changes {
		self.session.connected = connected;
		Changes {
			session: true,
			..Changes::default()
		}
	}

	fn guild_loaded_changes(&self, guild_id: Snowflake) -> Changes {
		let mut changes = Changes {
			guild_list: true,
			..Changes::default()
		};
		changes.channel_lists.insert(guild_id);
		changes.members.insert(guild_id);
		changes.voice.insert(guild_id);
		changes
	}

	fn permissions_changed(&self, guild_id: Snowflake, changes: &mut Changes) {
		changes.guild_list = true;
		changes.channel_lists.insert(guild_id);
		changes.members.insert(guild_id);
	}

	fn message_create(&mut self, message: &Message) -> Changes {
		let channel_id = message.channel_id;
		let guild_id = message.guild_id.or_else(|| self.channel_guild(channel_id));
		self.ingest_message_people(guild_id, message);
		let mut changes = Changes::default();
		if let Some(cache) = self.messages.get_mut(&channel_id) {
			let pending = message.nonce.as_ref().and_then(|nonce| cache.take_pending(nonce));
			let confirmed = pending.is_some();
			let shared = Arc::new(message.clone());
			if cache.push_live(Arc::clone(&shared)) {
				self.unrendered.push(shared);
				changes.messages.insert(channel_id);
			} else if confirmed {
				changes.messages.insert(channel_id);
			}
			if let Some(pending) = pending {
				if let Some(by) = pending.plugin {
					self.sent_by_plugin.insert(message.id, by);
				}
				if let Some(by) = pending.altered_by {
					self.altered.insert(message.id, by);
				}
			}
		}
		if let Some(channel) = self.channel_mut(channel_id)
			&& channel.last_message_id < Some(message.id)
		{
			channel.last_message_id = Some(message.id);
			if is_thread(channel.kind) {
				channel.message_count = Some(channel.message_count.unwrap_or(0) + 1);
				changes.loaded.insert(channel.parent_id.unwrap_or_default());
			}
		}
		if let Some(users) = self.typing.get_mut(&channel_id)
			&& users.remove(&message.author.id).is_some()
		{
			changes.typing.insert(channel_id);
		}
		let is_selected = self
			.selected
			.is_some_and(|selected| selected.channel_id == channel_id);
		if Some(message.author.id) == self.current_user_id() {
			self.set_read_state(channel_id, message.id, 0);
		} else if !is_selected && (guild_id.is_none() || self.mentions_me(message, guild_id)) {
			self.bump_mentions(channel_id);
		}
		changes.read_state = true;
		self.mark_channel_dirty(channel_id, &mut changes);
		changes
	}

	fn message_update(&mut self, update: &MessageUpdate) -> Changes {
		if let Some(author) = &update.author {
			intern_user(&mut self.users, author);
		}
		let changes = self.edit_message(update.channel_id, update.id, |message| {
			apply_update(message, update)
		});
		if update.content.is_some()
			&& let Some(edited) = self
				.messages
				.get(&update.channel_id)
				.and_then(|cache| cache.by_id.get(&update.id))
		{
			self.unrendered.push(Arc::clone(edited));
		}
		changes
	}

	fn edit_message(
		&mut self,
		channel_id: Snowflake,
		message_id: Snowflake,
		edit: impl FnOnce(&mut Message),
	) -> Changes {
		let mut changes = Changes::default();
		if let Some(cache) = self.messages.get_mut(&channel_id)
			&& cache.modify(message_id, edit)
		{
			changes.messages.insert(channel_id);
		}
		changes
	}

	fn messages_delete(&mut self, channel_id: Snowflake, ids: &[Snowflake]) -> Changes {
		let mut changes = Changes::default();
		if let Some(cache) = self.messages.get_mut(&channel_id) {
			let mut removed = false;
			for &id in ids {
				removed |= cache.remove(id);
			}
			if removed {
				changes.messages.insert(channel_id);
			}
		}
		changes
	}

	fn message_ack(&mut self, ack: &MessageAck) -> Changes {
		let mentions = ack
			.mention_count
			.map_or(0, |count| u32::try_from(count).unwrap_or(0));
		self.set_read_state(ack.channel_id, ack.message_id, mentions);
		let mut changes = Changes {
			read_state: true,
			..Changes::default()
		};
		self.mark_channel_dirty(ack.channel_id, &mut changes);
		changes
	}

	fn upsert_channel(&mut self, channel: &Channel) -> Changes {
		let mut changes = Changes::default();
		let Some(guild_id) = channel.guild_id else {
			self.store_private_channel(channel);
			changes.private_channels = true;
			return changes;
		};
		let Some(guild) = self.guilds.get_mut(&guild_id) else {
			return changes;
		};
		let mut stored = channel.clone();
		if stored.last_message_id.is_none() {
			stored.last_message_id = guild
				.channel(stored.id)
				.and_then(|existing| existing.last_message_id);
		}
		if stored.member.is_none() {
			stored.member = guild
				.channel(stored.id)
				.and_then(|existing| existing.member.clone());
		}
		self.channel_guilds.insert(stored.id, Some(guild_id));
		guild.insert_channel(stored);
		if let Some(parent_id) = channel.parent_id {
			changes.loaded.insert(parent_id);
		}
		changes.channel_lists.insert(guild_id);
		changes.guild_list = true;
		changes
	}

	fn remove_channel(&mut self, channel_id: Snowflake) -> Changes {
		let mut changes = Changes::default();
		self.mark_channel_dirty(channel_id, &mut changes);
		match self.channel_guilds.remove(&channel_id) {
			Some(Some(guild_id)) => {
				if let Some(guild) = self.guilds.get_mut(&guild_id) {
					guild.remove_channel(channel_id);
				}
			}
			Some(None) => {
				self.private_channels.shift_remove(&channel_id);
			}
			None => {}
		}
		if self.messages.remove(&channel_id).is_some() {
			changes.messages.insert(channel_id);
		}
		self.typing.remove(&channel_id);
		if self.read_states.remove(&channel_id).is_some() {
			changes.read_state = true;
		}
		changes
	}

	fn thread_delete(&mut self, delete: &ThreadDelete) -> Changes {
		self.remove_channel(delete.id)
	}

	fn thread_list_sync(&mut self, sync: &ThreadListSync) -> Changes {
		let mut changes = Changes::default();
		let Some(guild) = self.guilds.get_mut(&sync.guild_id) else {
			return changes;
		};
		let stale: Vec<Snowflake> = guild
			.threads
			.values()
			.filter(|thread| match (&sync.channel_ids, thread.parent_id) {
				(None, _) => true,
				(Some(ids), Some(parent_id)) => ids.contains(&parent_id),
				(Some(_), None) => false,
			})
			.map(|thread| thread.id)
			.collect();
		for id in stale {
			guild.threads.shift_remove(&id);
			self.channel_guilds.remove(&id);
		}
		for thread in sync.threads.iter().filter(|thread| is_thread(thread.kind)) {
			let mut stored = thread.clone();
			stored.guild_id = Some(sync.guild_id);
			self.channel_guilds.insert(stored.id, Some(sync.guild_id));
			guild.insert_channel(stored);
		}
		changes.channel_lists.insert(sync.guild_id);
		changes.guild_list = true;
		for member in sync.members.iter() {
			self.set_thread_member(member, &mut changes);
		}
		changes
	}

	fn guild_delete(&mut self, guild: &UnavailableGuild) -> Changes {
		let mut changes = Changes {
			guild_list: true,
			..Changes::default()
		};
		changes.channel_lists.insert(guild.id);
		if guild.unavailable == Some(true) {
			if let Some(state) = self.guilds.get_mut(&guild.id) {
				state.unavailable = true;
			}
			return changes;
		}
		let Some(state) = self.guilds.shift_remove(&guild.id) else {
			return changes;
		};
		for id in state.channels.keys().chain(state.threads.keys()) {
			self.channel_guilds.remove(id);
			self.messages.remove(id);
			self.typing.remove(id);
			self.read_states.remove(id);
		}
		changes.messages.mark_all();
		changes.read_state = true;
		changes
	}

	fn upsert_guild_member(&mut self, member: &Member) -> Changes {
		let mut changes = Changes::default();
		let (Some(guild_id), Some(user)) = (member.guild_id, &member.user) else {
			return changes;
		};
		let Some(guild) = self.guilds.get_mut(&guild_id) else {
			return changes;
		};
		intern_user(&mut self.users, user);
		guild.members.insert(user.id, stripped_member(member));
		changes.members.insert(guild_id);
		if Some(user.id) == self.current_user.as_ref().map(|me| me.id) {
			self.permissions_changed(guild_id, &mut changes);
		}
		changes
	}

	fn members_chunk(&mut self, chunk: &GuildMembersChunk) -> Changes {
		self.ingest_members(chunk.guild_id, &chunk.members);
		let mut changes = Changes::default();
		changes.members.insert(chunk.guild_id);
		for presence in chunk.presences.iter() {
			if let Some(user_id) = self.apply_presence(presence) {
				changes.presence.insert(user_id);
			}
		}
		changes
	}

	fn role_changed(&mut self, change: &GuildRoleChange) -> Changes {
		let mut changes = Changes::default();
		if let Some(guild) = self.guilds.get_mut(&change.guild_id) {
			guild.roles.insert(change.role.id, change.role.clone());
			self.permissions_changed(change.guild_id, &mut changes);
		}
		changes
	}

	fn typing_start(&mut self, typing: &TypingStart) -> Changes {
		let mut changes = Changes::default();
		if Some(typing.user_id) == self.current_user_id() {
			return changes;
		}
		self.ingest_typing_member(typing);
		self.typing
			.entry(typing.channel_id)
			.or_default()
			.insert(typing.user_id, Instant::now());
		changes.typing.insert(typing.channel_id);
		changes
	}

	fn guild_settings_update(&mut self, settings: &UserGuildSettings) -> Changes {
		self.store_guild_settings(settings);
		let mut changes = Changes {
			guild_list: true,
			read_state: true,
			..Changes::default()
		};
		match settings.guild_id {
			Some(guild_id) => changes.channel_lists.insert(guild_id),
			None => changes.private_channels = true,
		}
		changes
	}

	fn voice_state_update(&mut self, voice: &VoiceState) -> Changes {
		let mut changes = Changes::default();
		let Some(guild_id) = voice.guild_id else {
			return self.call_voice_state(voice);
		};
		if let Some(user) = voice.member.as_ref().and_then(|member| member.user.as_ref()) {
			intern_user(&mut self.users, user);
		}
		let Some(guild) = self.guilds.get_mut(&guild_id) else {
			return changes;
		};
		if voice.channel_id.is_some() {
			guild.voice_states.insert(voice.user_id, voice.clone());
		} else {
			guild.voice_states.remove(&voice.user_id);
		}
		changes.voice.insert(guild_id);
		changes.channel_lists.insert(guild_id);
		changes
	}

	fn member_list_update(&mut self, update: &GuildMemberListUpdate) -> Changes {
		let guild_id = update.guild_id;
		let mut changes = Changes::default();
		let Some(guild) = self.guilds.get_mut(&guild_id) else {
			return changes;
		};
		if guild.member_list.id != update.id {
			guild.member_list.reset(&update.id);
		}
		for op in &update.ops {
			self.apply_member_list_op(guild_id, op, &mut changes);
		}
		if let Some(guild) = self.guilds.get_mut(&guild_id) {
			let list = &mut guild.member_list;
			if !update.groups.is_empty() {
				list.groups = update.groups.to_vec();
			}
			list.member_count = update.member_count.unwrap_or(list.member_count);
			list.online_count = update.online_count.unwrap_or(list.online_count);
		}
		changes.members.insert(guild_id);
		changes
	}

	fn apply_member_list_op(&mut self, guild_id: Snowflake, op: &Value, changes: &mut Changes) {
		let Some(kind) = op.get("op").and_then(Value::as_str) else {
			return;
		};
		match kind {
			"SYNC" => {
				let Some((start, end)) = op_range(op) else {
					return;
				};
				let items: Vec<MemberSlot> = op
					.get("items")
					.and_then(Value::as_array)
					.into_iter()
					.flatten()
					.map(|item| self.slot_from_item(guild_id, item, changes))
					.collect();
				if let Some(guild) = self.guilds.get_mut(&guild_id) {
					guild.member_list.sync(start, end, items);
				}
			}
			"INSERT" | "UPDATE" => {
				let Some(index) = op_index(op) else {
					return;
				};
				let slot = match op.get("item") {
					Some(item) => self.slot_from_item(guild_id, item, changes),
					None => MemberSlot::Empty,
				};
				if let Some(guild) = self.guilds.get_mut(&guild_id) {
					if kind == "INSERT" {
						guild.member_list.insert(index, slot);
					} else {
						guild.member_list.update(index, slot);
					}
				}
			}
			"DELETE" => {
				if let (Some(index), Some(guild)) = (op_index(op), self.guilds.get_mut(&guild_id)) {
					guild.member_list.delete(index);
				}
			}
			"INVALIDATE" => {
				if let (Some((start, end)), Some(guild)) = (op_range(op), self.guilds.get_mut(&guild_id)) {
					guild.member_list.invalidate(start, end);
				}
			}
			_ => tracing::debug!(%guild_id, "ignored unknown member list op"),
		}
	}

	fn slot_from_item(&mut self, guild_id: Snowflake, item: &Value, changes: &mut Changes) -> MemberSlot {
		let Ok(raw) = serde_json::from_str::<RawListItem>(&item.to_string()) else {
			return MemberSlot::Empty;
		};
		if let Some(group) = raw.group {
			return MemberSlot::Group {
				id: group.id,
				count: group.count,
			};
		}
		let Some(raw_member) = raw.member else {
			return MemberSlot::Empty;
		};
		let Ok(member) = serde_json::from_str::<Member>(raw_member.get()) else {
			return MemberSlot::Empty;
		};
		let Some(user) = &member.user else {
			return MemberSlot::Empty;
		};
		let user_id = user.id;
		intern_user(&mut self.users, user);
		if let Some(guild) = self.guilds.get_mut(&guild_id) {
			guild.members.insert(user_id, stripped_member(&member));
		}
		if let Ok(RawListPresence {
			presence: Some(presence),
		}) = serde_json::from_str(raw_member.get())
		{
			self.set_presence(user_id, &presence);
			changes.presence.insert(user_id);
		}
		MemberSlot::Member(user_id)
	}
}

fn op_range(op: &Value) -> Option<(usize, usize)> {
	let range = op.get("range")?.as_array()?;
	let start = usize::try_from(range.first()?.as_u64()?).ok()?;
	let end = usize::try_from(range.get(1)?.as_u64()?).ok()?;
	Some((start, end))
}

fn op_index(op: &Value) -> Option<usize> {
	usize::try_from(op.get("index")?.as_u64()?).ok()
}

impl State {
	fn ingest_typing_member(&mut self, typing: &TypingStart) {
		let Some(member) = &typing.member else {
			return;
		};
		if let Some(user) = &member.user {
			intern_user(&mut self.users, user);
		}
		let Some(guild) = typing.guild_id.and_then(|id| self.guilds.get_mut(&id)) else {
			return;
		};
		guild
			.members
			.entry(typing.user_id)
			.or_insert_with(|| stripped_member(member));
	}
}
