use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Local, NaiveDate, TimeDelta, Utc};
use protocol::{
	ActivityKind, Channel, ChannelFlags, ChannelType, Message, MessageType, Permissions, Presence,
	RelationshipKind, Snowflake, StatusKind, User, UserGuildSettings,
};
use smallvec::SmallVec;

use crate::guild::{GuildState, is_voice};
use crate::member_list::MemberSlot;
use crate::messages::{ChannelMessages, PendingMessage};
use crate::permissions::compute_permissions;
use crate::state::State;
use crate::views::{
	ChannelRow, ChannelTreeNode, DmRow, GuildSummary, JoinBlock, MemberListRow, MemberRow, MessageRow,
	MessageView, PendingUpload, RenderedMessage, RequestRow, UnreadSince,
};

const GROUPING_WINDOW: TimeDelta = TimeDelta::minutes(7);
const UNREAD_COUNT_CAP: usize = 50;

struct RowContext<'a> {
	settings: Option<&'a UserGuildSettings>,
	me: Option<Snowflake>,
	now: DateTime<Utc>,
}

fn sent_at(message: &Message) -> DateTime<Utc> {
	message.timestamp.unwrap_or_else(|| message.id.timestamp())
}

fn local_day(message: &Message) -> NaiveDate {
	sent_at(message).with_timezone(&Local).date_naive()
}

fn is_plain(message: &Message) -> bool {
	message.kind == MessageType::Default && message.referenced_message.is_none() && !message.is_forward()
}

fn groups_with(previous: &Message, message: &Message) -> bool {
	previous.author.id == message.author.id
		&& is_plain(previous)
		&& is_plain(message)
		&& sent_at(message) - sent_at(previous) < GROUPING_WINDOW
}

fn role_color_value(role: &protocol::Role) -> Option<u32> {
	let color = if role.color != 0 {
		role.color
	} else {
		role.colors
			.as_ref()
			.and_then(|colors| colors.primary_color)
			.unwrap_or(0)
	};
	u32::try_from(color).ok().filter(|&color| color != 0)
}

fn top_role_color(guild: &GuildState, roles: &[Snowflake]) -> Option<u32> {
	roles
		.iter()
		.filter_map(|id| guild.roles.get(id))
		.filter(|role| role_color_value(role).is_some())
		.max_by_key(|role| role.position)
		.and_then(role_color_value)
}

fn hoisted_role(guild: &GuildState, roles: &[Snowflake]) -> Option<Snowflake> {
	roles
		.iter()
		.filter_map(|id| guild.roles.get(id))
		.filter(|role| role.hoist)
		.max_by_key(|role| role.position)
		.map(|role| role.id)
}

fn channel_sort_key(channel: &Channel) -> (bool, i64, Snowflake) {
	(is_voice(channel.kind), channel.position.unwrap_or(0), channel.id)
}

fn count_of(value: i64) -> u32 {
	u32::try_from(value).unwrap_or(0)
}

fn is_obfuscated(channel: &Channel) -> bool {
	channel.flags.contains(ChannelFlags::CHANNEL_OBFUSCATED)
}

fn voice_members(guild: &GuildState, channel: &Channel) -> Vec<Snowflake> {
	if !is_voice(channel.kind) {
		return Vec::new();
	}
	let mut members: Vec<Snowflake> = guild
		.voice_states
		.values()
		.filter(|voice| voice.channel_id == Some(channel.id))
		.map(|voice| voice.user_id)
		.collect();
	members.sort_unstable();
	members
}

fn join_block(
	permissions: Permissions,
	user_limit: u32,
	occupants: &[Snowflake],
	me: Option<Snowflake>,
) -> Option<JoinBlock> {
	if !permissions.contains(Permissions::CONNECT) {
		return Some(JoinBlock::NoPermission);
	}
	let full = user_limit > 0
		&& occupants.len() >= user_limit as usize
		&& !permissions.contains(Permissions::MOVE_MEMBERS)
		&& !me.is_some_and(|me| occupants.contains(&me));
	full.then_some(JoinBlock::Full)
}

fn activity_text(presence: &Presence) -> Option<String> {
	let custom = presence
		.activities
		.iter()
		.filter(|activity| activity.kind == ActivityKind::Custom)
		.find_map(|activity| activity.state.clone().filter(|state| !state.is_empty()));
	if custom.is_some() {
		return custom;
	}
	let activity = presence
		.activities
		.iter()
		.find(|activity| activity.kind != ActivityKind::Custom)?;
	let text = match activity.kind {
		ActivityKind::Playing => format!("Playing {}", activity.name),
		ActivityKind::Streaming => format!("Streaming {}", activity.name),
		ActivityKind::Listening => format!("Listening to {}", activity.name),
		ActivityKind::Watching => format!("Watching {}", activity.name),
		ActivityKind::Competing => format!("Competing in {}", activity.name),
		_ => activity.name.clone(),
	};
	Some(text)
}

static NAMES_MASKED: AtomicBool = AtomicBool::new(false);

const MASKED_NAME: &str = "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}";

pub fn set_names_masked(masked: bool) -> bool {
	NAMES_MASKED.swap(masked, Ordering::Relaxed) != masked
}

fn user_display(user: &User) -> String {
	let custom = user.global_name.as_deref().filter(|name| !name.is_empty());
	match custom {
		Some(name) => name.to_owned(),
		None if NAMES_MASKED.load(Ordering::Relaxed) => MASKED_NAME.to_owned(),
		None => user.username.clone(),
	}
}

impl State {
	pub fn display_name(&self, guild_id: Option<Snowflake>, user_id: Snowflake) -> String {
		let nick = guild_id
			.and_then(|id| self.guilds.get(&id))
			.and_then(|guild| guild.members.get(&user_id))
			.and_then(|member| member.nick.as_deref())
			.filter(|nick| !nick.is_empty());
		if let Some(nick) = nick {
			return nick.to_owned();
		}
		if guild_id.is_none()
			&& let Some(nick) = self.friend_nickname(user_id)
		{
			return nick.to_owned();
		}
		match self.users.get(&user_id) {
			Some(user) => user_display(user),
			None => user_id.to_string(),
		}
	}

	pub fn friend_nickname(&self, user_id: Snowflake) -> Option<&str> {
		self.relationships
			.get(&user_id)
			.filter(|relationship| relationship.kind == RelationshipKind::Friend)
			.and_then(|relationship| relationship.nickname.as_deref())
			.filter(|nick| !nick.is_empty())
	}

	pub fn role_color(&self, guild_id: Snowflake, user_id: Snowflake) -> Option<u32> {
		let guild = self.guilds.get(&guild_id)?;
		top_role_color(guild, &guild.members.get(&user_id)?.roles)
	}

	pub fn avatar_hash(&self, guild_id: Option<Snowflake>, user_id: Snowflake) -> Option<&str> {
		let member_avatar = guild_id
			.and_then(|id| self.guilds.get(&id))
			.and_then(|guild| guild.members.get(&user_id))
			.and_then(|member| member.avatar.as_deref());
		member_avatar.or_else(|| self.users.get(&user_id)?.avatar.as_deref())
	}

	pub fn activity_summary(&self, user_id: Snowflake) -> Option<String> {
		self.presences.get(&user_id).and_then(activity_text)
	}

	pub fn guild_summaries(&self) -> Vec<GuildSummary> {
		let now = Utc::now();
		self.guilds
			.values()
			.map(|guild| self.guild_summary_at(guild, now))
			.collect()
	}

	pub fn guild_summary(&self, guild_id: Snowflake) -> Option<GuildSummary> {
		Some(self.guild_summary_at(self.guilds.get(&guild_id)?, Utc::now()))
	}

	fn guild_summary_at(&self, guild: &GuildState, now: DateTime<Utc>) -> GuildSummary {
		let status = self.guild_status(guild, now);
		GuildSummary {
			id: guild.id,
			name: guild.name.clone(),
			icon: guild.icon.clone(),
			unread: status.unread,
			mentions: status.mentions,
			muted: status.muted,
			folder: self.guild_folder_index(guild.id),
		}
	}

	pub fn default_channel(&self, guild_id: Snowflake) -> Option<Snowflake> {
		self.guild_channel_tree(guild_id)
			.iter()
			.flat_map(|node| node.children.iter())
			.find(|channel| {
				!channel.locked
					&& matches!(
						channel.kind,
						ChannelType::GuildText | ChannelType::GuildAnnouncement
					)
			})
			.map(|channel| channel.id)
	}

	pub fn guild_channel_tree(&self, guild_id: Snowflake) -> Vec<ChannelTreeNode> {
		let Some(guild) = self.guilds.get(&guild_id) else {
			return Vec::new();
		};
		let context = self.row_context(Some(guild_id));
		let mut categories: Vec<&Channel> = guild
			.channels
			.values()
			.filter(|channel| channel.kind == ChannelType::GuildCategory && !is_obfuscated(channel))
			.collect();
		categories.sort_by_key(|channel| (channel.position.unwrap_or(0), channel.id));
		let mut grouped: HashMap<Option<Snowflake>, Vec<&Channel>> = HashMap::new();
		for channel in guild
			.channels
			.values()
			.filter(|channel| channel.kind != ChannelType::GuildCategory && !is_obfuscated(channel))
		{
			let parent = channel.parent_id.filter(|id| {
				guild
					.channels
					.get(id)
					.is_some_and(|parent| parent.kind == ChannelType::GuildCategory)
			});
			grouped.entry(parent).or_default().push(channel);
		}
		let selected = self.selected.map(|selection| selection.channel_id);
		let mut threads: HashMap<Snowflake, Vec<&Channel>> = HashMap::new();
		for thread in guild.threads.values().filter(|thread| {
			(thread.member.is_some() || Some(thread.id) == selected)
				&& thread
					.thread_metadata
					.as_ref()
					.is_none_or(|metadata| !metadata.archived)
		}) {
			let Some(parent_id) = thread.parent_id else {
				continue;
			};
			if guild.channels.get(&parent_id).is_some_and(|parent| {
				!matches!(parent.kind, ChannelType::GuildForum | ChannelType::GuildMedia)
			}) {
				threads.entry(parent_id).or_default().push(thread);
			}
		}
		for group in threads.values_mut() {
			group.sort_by_key(|thread| thread.id);
		}
		let mut nodes = Vec::with_capacity(categories.len() + 1);
		if let Some(loose) = grouped.remove(&None) {
			nodes.push(ChannelTreeNode {
				category: None,
				children: self.sorted_rows(guild, loose, &threads, &context),
				collapsed: false,
			});
		}
		for category in categories {
			let children = grouped.remove(&Some(category.id)).unwrap_or_default();
			nodes.push(ChannelTreeNode {
				category: Some(self.channel_row(guild, category, &context)),
				children: self.sorted_rows(guild, children, &threads, &context),
				collapsed: self.collapsed_categories.contains(&category.id),
			});
		}
		nodes
	}

	fn row_context(&self, guild_id: Option<Snowflake>) -> RowContext<'_> {
		RowContext {
			settings: self.guild_settings(guild_id),
			me: self.current_user_id(),
			now: Utc::now(),
		}
	}

	fn sorted_rows(
		&self,
		guild: &GuildState,
		mut channels: Vec<&Channel>,
		threads: &HashMap<Snowflake, Vec<&Channel>>,
		context: &RowContext,
	) -> Vec<ChannelRow> {
		channels.sort_by_key(|channel| channel_sort_key(channel));
		let mut rows = Vec::with_capacity(channels.len());
		for channel in channels {
			rows.push(self.channel_row(guild, channel, context));
			if let Some(children) = threads.get(&channel.id) {
				rows.extend(
					children
						.iter()
						.map(|thread| self.channel_row(guild, thread, context)),
				);
			}
		}
		rows
	}

	fn channel_row(&self, guild: &GuildState, channel: &Channel, context: &RowContext) -> ChannelRow {
		let permissions = context.me.map_or(Permissions::all(), |me| {
			compute_permissions(guild, Some(channel), me, context.now)
		});
		let viewable = permissions.contains(Permissions::VIEW_CHANNEL);
		let status = self.channel_status(channel, context.settings, viewable, context.now);
		let voice_members = voice_members(guild, channel);
		let user_limit = count_of(channel.user_limit.unwrap_or(0));
		let join_block = is_voice(channel.kind)
			.then(|| join_block(permissions, user_limit, &voice_members, context.me))
			.flatten();
		ChannelRow {
			id: channel.id,
			kind: channel.kind,
			name: channel.name.clone().unwrap_or_default(),
			parent_id: channel.parent_id,
			position: channel.position.unwrap_or(0),
			unread: status.unread,
			mentions: status.mentions,
			muted: status.muted,
			locked: !viewable,
			member: channel.member.is_some(),
			user_limit,
			voice_status: channel.voice_status().map(str::to_owned),
			join_block,
			voice_members,
		}
	}

	pub fn voice_join_block(&self, channel_id: Snowflake) -> Option<JoinBlock> {
		let guild = self.guilds.get(&self.channel_guild(channel_id)?)?;
		let channel = guild.channel(channel_id)?;
		join_block(
			self.my_permissions(channel_id),
			count_of(channel.user_limit.unwrap_or(0)),
			&voice_members(guild, channel),
			self.current_user_id(),
		)
	}

	pub fn dm_rows(&self) -> Vec<DmRow> {
		let now = Utc::now();
		let mut rows: Vec<DmRow> = self
			.private_channels
			.values()
			.filter(|channel| !channel.is_request())
			.map(|channel| self.dm_row_at(channel, now))
			.collect();
		rows.sort_by_key(|row| Reverse((row.last_message_id, row.channel_id)));
		rows
	}

	pub fn unread_dm_rows(&self) -> Vec<DmRow> {
		let mut rows = self.dm_rows();
		rows.retain(|row| !row.muted && (row.unread || row.mentions > 0));
		rows
	}

	pub fn dm_row(&self, channel_id: Snowflake) -> Option<DmRow> {
		let channel = self.private_channels.get(&channel_id)?;
		Some(self.dm_row_at(channel, Utc::now()))
	}

	fn dm_row_at(&self, channel: &Channel, now: DateTime<Utc>) -> DmRow {
		let status = self.channel_status(channel, self.global_settings.as_ref(), true, now);
		DmRow {
			channel_id: channel.id,
			name: self.dm_name(channel),
			icon: channel.icon.clone(),
			recipients: channel.recipient_ids.clone(),
			unread: status.unread,
			muted: status.muted,
			mentions: status.mentions,
			last_message_id: channel.last_message_id,
		}
	}

	pub fn message_requests(&self) -> Vec<RequestRow> {
		let mut rows: Vec<RequestRow> = self
			.private_channels
			.values()
			.filter_map(|channel| self.request_row(channel))
			.collect();
		rows.sort_by_key(|row| Reverse((row.person.since, row.channel_id)));
		rows
	}

	pub fn message_request(&self, channel_id: Snowflake) -> Option<RequestRow> {
		self.request_row(self.private_channels.get(&channel_id)?)
	}

	pub fn message_request_count(&self) -> usize {
		self.private_channels
			.values()
			.filter(|channel| channel.is_request())
			.count()
	}

	fn request_row(&self, channel: &Channel) -> Option<RequestRow> {
		if !channel.is_request() {
			return None;
		}
		let user_id = *channel.recipient_ids.first()?;
		Some(RequestRow {
			channel_id: channel.id,
			spam: channel.is_spam_request(),
			person: self.person_row(user_id, None, channel.is_message_request_timestamp),
		})
	}

	pub fn dm_name(&self, channel: &Channel) -> String {
		if let Some(name) = channel.name.as_deref().filter(|name| !name.is_empty()) {
			return name.to_owned();
		}
		let names: Vec<String> = channel
			.recipient_ids
			.iter()
			.map(|id| self.display_name(None, *id))
			.collect();
		names.join(", ")
	}

	pub fn channel_rows(&self, channel_id: Snowflake) -> Vec<MessageRow> {
		self.channel_rows_after(channel_id, None)
	}

	pub fn channel_rows_after(&self, channel_id: Snowflake, anchor: Option<Snowflake>) -> Vec<MessageRow> {
		let Some(cache) = self.messages.get(&channel_id) else {
			return vec![MessageRow::Loading];
		};
		let guild = self
			.channel_guild(channel_id)
			.and_then(|guild_id| self.guilds.get(&guild_id));
		let me = self.current_user_id();
		let last_read = anchor.or_else(|| {
			self.read_states
				.get(&channel_id)
				.and_then(|state| state.last_message_id)
		});
		let mut rows = Vec::with_capacity(cache.ordered.len() + cache.pending.len() + 4);
		rows.push(if cache.reached_start {
			MessageRow::HistoryStart
		} else {
			MessageRow::Loading
		});
		let confirmed = cache.ordered.iter().map(|message| (message, None));
		let pending = cache
			.pending
			.iter()
			.map(|pending| (&pending.message, Some(pending)));
		let mut previous: Option<(&Message, NaiveDate)> = None;
		let mut separated_day: Option<NaiveDate> = None;
		let mut marker_placed = false;
		let mut blocked_run: Option<bool> = None;
		let mut run_between = false;
		for (message, pending) in confirmed.chain(pending) {
			if pending.is_none() && cache.is_hidden(message.id) {
				continue;
			}
			if pending.is_none() && self.is_blocked(message.author.id) {
				let revealed = *blocked_run.get_or_insert_with(|| {
					let (mut count, mut last, mut revealed) = (0, message.id, false);
					for member in self.blocked_run(cache, message.id) {
						count += 1;
						last = member.id;
						revealed |= self.revealed_blocked.contains(&member.id);
					}
					let day = local_day(message);
					if separated_day != Some(day) {
						rows.push(MessageRow::DaySeparator(day));
						separated_day = Some(day);
					}
					rows.push(MessageRow::BlockedRun {
						first: message.id,
						last,
						count,
						revealed,
					});
					run_between = true;
					revealed
				});
				if !revealed {
					continue;
				}
			} else {
				blocked_run = None;
			}
			let day = local_day(message);
			if separated_day != Some(day) {
				rows.push(MessageRow::DaySeparator(day));
				separated_day = Some(day);
			}
			let is_unread = pending.is_none()
				&& last_read.is_some_and(|read| message.id > read)
				&& Some(message.author.id) != me;
			if !marker_placed && is_unread && (previous.is_some() || cache.reached_start) {
				rows.push(MessageRow::UnreadMarker);
				marker_placed = true;
			}
			let grouped = !run_between
				&& previous.is_some_and(|(earlier, earlier_day)| {
					earlier_day == day && groups_with(earlier, message)
				});
			run_between = false;
			rows.push(MessageRow::Message(
				self.message_view(guild, cache, message, grouped, pending),
			));
			previous = Some((message.as_ref(), day));
		}
		rows
	}

	pub(crate) fn blocked_run<'a>(
		&'a self,
		cache: &'a ChannelMessages,
		first: Snowflake,
	) -> impl Iterator<Item = &'a Arc<Message>> {
		let start = cache.ordered.partition_point(|message| message.id < first);
		cache.ordered[start..]
			.iter()
			.filter(|message| !cache.is_hidden(message.id))
			.take_while(|message| self.is_blocked(message.author.id))
	}

	pub fn unread_since(&self, channel_id: Snowflake, anchor: Snowflake) -> Option<UnreadSince> {
		let cache = self.messages.get(&channel_id)?;
		let me = self.current_user_id();
		let start = cache.ordered.partition_point(|message| message.id <= anchor);
		let mut unread = cache.ordered[start..]
			.iter()
			.filter(|message| Some(message.author.id) != me && !cache.is_hidden(message.id));
		let first = unread.next()?;
		let gap = start == 0 && !cache.reached_start;
		let count = 1 + unread.take(UNREAD_COUNT_CAP).count();
		Some(UnreadSince {
			count: count.min(UNREAD_COUNT_CAP),
			more: gap || count > UNREAD_COUNT_CAP,
			jump_to: if gap { anchor } else { first.id },
			since: if gap { anchor.timestamp() } else { sent_at(first) },
		})
	}

	fn starter_message(&self, message: &Message) -> Option<(Arc<Message>, Option<Arc<RenderedMessage>>)> {
		if message.kind != MessageType::ThreadStarterMessage {
			return None;
		}
		let reference = message.message_reference.as_ref()?;
		let live = self
			.messages
			.get(&reference.channel_id?)
			.zip(reference.message_id)
			.and_then(|(cache, id)| cache.by_id.get(&id).map(|source| (cache, id, source)));
		let (source, rendered) = match live {
			Some((cache, id, _)) if cache.is_deleted(id) => return None,
			Some((cache, id, source)) => (source.as_ref().clone(), cache.rendered.get(&id).cloned()),
			None => (message.referenced_message.as_ref()?.as_ref().clone(), None),
		};
		let starter = Arc::new(Message {
			id: message.id,
			channel_id: message.channel_id,
			guild_id: message.guild_id,
			kind: message.kind,
			message_reference: message.message_reference.clone(),
			referenced_message: None,
			pinned: false,
			reactions: Default::default(),
			components: Default::default(),
			poll: None,
			thread: None,
			..source
		});
		Some((starter, rendered))
	}

	fn message_view(
		&self,
		guild: Option<&GuildState>,
		cache: &ChannelMessages,
		message: &Arc<Message>,
		grouped_with_previous: bool,
		pending: Option<&PendingMessage>,
	) -> MessageView {
		let starter = self.starter_message(message);
		let message = starter.as_ref().map_or(message, |(starter, _)| starter);
		let author = &message.author;
		let member = guild
			.and_then(|guild| guild.members.get(&author.id))
			.or(message.member.as_ref());
		let member_roles: SmallVec<[Snowflake; 4]> = member
			.map(|member| member.roles.iter().copied().collect())
			.unwrap_or_default();
		let author_display = member
			.and_then(|member| member.nick.as_deref())
			.filter(|nick| !nick.is_empty())
			.map_or_else(|| user_display(author), str::to_owned);
		let over = self.user_override(guild.map(|guild| guild.id), author.id);
		let role_color = guild.and_then(|guild| top_role_color(guild, &member_roles));
		let (rendered, plugin, altered_by) = match pending {
			Some(pending) => (
				pending.rendered.clone(),
				pending.plugin.clone(),
				pending.altered_by.clone(),
			),
			None => (
				match &starter {
					Some((_, rendered)) => rendered.clone(),
					None => cache.rendered.get(&message.id).cloned(),
				},
				self.sent_by_plugin(message.id).map(str::to_owned),
				self.altered_by(message.id).map(str::to_owned),
			),
		};
		MessageView {
			message: Arc::clone(message),
			grouped_with_previous,
			pending: pending.is_some(),
			failed: pending.is_some_and(|pending| pending.failed),
			retryable: pending.is_some_and(|pending| pending.retryable),
			upload: pending
				.filter(|pending| !pending.files.is_empty())
				.map(|pending| {
					Box::new(PendingUpload {
						files: Arc::clone(&pending.files),
						uploaded: pending.uploaded,
					})
				}),
			failure_reason: pending.and_then(|pending| pending.failure_reason.clone()),
			plugin,
			altered_by,
			rendered,
			deleted: pending.is_none() && cache.is_deleted(message.id),
			author_color: over.and_then(|over| over.color).or(role_color),
			author_display: over.and_then(|over| over.name.clone()).unwrap_or(author_display),
			author_badge: over.and_then(|over| over.badge.clone()),
			member_roles,
			mentions_me: self.mentions_me(message, guild.map(|guild| guild.id)),
		}
	}

	pub fn member_rows(&self, guild_id: Snowflake) -> Vec<MemberListRow> {
		self.member_rows_at(guild_id)
			.into_iter()
			.map(|(_, row)| row)
			.collect()
	}

	pub fn member_rows_at(&self, guild_id: Snowflake) -> Vec<(usize, MemberListRow)> {
		let Some(guild) = self.guilds.get(&guild_id) else {
			return Vec::new();
		};
		let list = &guild.member_list;
		list.slots
			.iter()
			.enumerate()
			.filter_map(|(index, slot)| {
				let row = match slot {
					MemberSlot::Empty => return None,
					MemberSlot::Group { id, count } => MemberListRow::Group {
						id: id.clone(),
						name: group_name(guild, id),
						count: count_of(list.group_count(id).unwrap_or(*count)),
					},
					MemberSlot::Member(user_id) => MemberListRow::Member(self.member_row(guild, *user_id)),
				};
				Some((index, row))
			})
			.collect()
	}

	pub fn member_row_count(&self, guild_id: Snowflake) -> usize {
		self.guilds
			.get(&guild_id)
			.map_or(0, |guild| guild.member_list.row_count())
	}

	pub fn member_row(&self, guild: &GuildState, user_id: Snowflake) -> MemberRow {
		let roles = guild
			.members
			.get(&user_id)
			.map_or(&[][..], |member| member.roles.as_slice());
		let presence = self.presences.get(&user_id);
		let over = self.user_override(Some(guild.id), user_id);
		MemberRow {
			user_id,
			display: over
				.and_then(|over| over.name.clone())
				.unwrap_or_else(|| self.display_name(Some(guild.id), user_id)),
			color: over
				.and_then(|over| over.color)
				.or_else(|| top_role_color(guild, roles)),
			badge: over.and_then(|over| over.badge.clone()),
			status: presence.map_or(StatusKind::Offline, |presence| presence.status.clone()),
			hoisted_role: hoisted_role(guild, roles),
			activity: presence.and_then(activity_text),
		}
	}
}

fn group_name(guild: &GuildState, id: &str) -> String {
	match id {
		"online" => "Online".to_owned(),
		"offline" => "Offline".to_owned(),
		_ => id
			.parse::<u64>()
			.ok()
			.and_then(|role_id| guild.roles.get(&Snowflake(role_id)))
			.map_or_else(|| id.to_owned(), |role| role.name.clone()),
	}
}
