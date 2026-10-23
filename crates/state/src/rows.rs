use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Local, NaiveDate, TimeDelta, Utc};
use protocol::{
	ActivityKind, Channel, ChannelType, Message, MessageType, Permissions, Presence, Snowflake, StatusKind,
	User, UserGuildSettings,
};
use smallvec::SmallVec;

use crate::guild::{GuildState, is_voice};
use crate::member_list::MemberSlot;
use crate::messages::{ChannelMessages, PendingMessage};
use crate::permissions::compute_permissions;
use crate::state::State;
use crate::views::{
	ChannelRow, ChannelTreeNode, DmRow, GuildSummary, MemberListRow, MemberRow, MessageRow, MessageView,
};

const GROUPING_WINDOW: TimeDelta = TimeDelta::minutes(7);

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
	message.kind == MessageType::Default && message.referenced_message.is_none()
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

fn activity_text(presence: &Presence) -> Option<String> {
	let activity = presence.activities.first()?;
	let text = match activity.kind {
		ActivityKind::Custom => return activity.state.clone().filter(|state| !state.is_empty()),
		ActivityKind::Playing => format!("Playing {}", activity.name),
		ActivityKind::Streaming => format!("Streaming {}", activity.name),
		ActivityKind::Listening => format!("Listening to {}", activity.name),
		ActivityKind::Watching => format!("Watching {}", activity.name),
		ActivityKind::Competing => format!("Competing in {}", activity.name),
		_ => activity.name.clone(),
	};
	Some(text)
}

fn user_display(user: &User) -> String {
	user.global_name
		.as_deref()
		.filter(|name| !name.is_empty())
		.unwrap_or(&user.username)
		.to_owned()
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
		match self.users.get(&user_id) {
			Some(user) => user_display(user),
			None => user_id.to_string(),
		}
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

	pub fn guild_channel_tree(&self, guild_id: Snowflake) -> Vec<ChannelTreeNode> {
		let Some(guild) = self.guilds.get(&guild_id) else {
			return Vec::new();
		};
		let context = self.row_context(Some(guild_id));
		let mut categories: Vec<&Channel> = guild
			.channels
			.values()
			.filter(|channel| channel.kind == ChannelType::GuildCategory)
			.collect();
		categories.sort_by_key(|channel| (channel.position.unwrap_or(0), channel.id));
		let mut grouped: HashMap<Option<Snowflake>, Vec<&Channel>> = HashMap::new();
		for channel in guild
			.channels
			.values()
			.filter(|channel| channel.kind != ChannelType::GuildCategory)
		{
			let parent = channel.parent_id.filter(|id| {
				guild
					.channels
					.get(id)
					.is_some_and(|parent| parent.kind == ChannelType::GuildCategory)
			});
			grouped.entry(parent).or_default().push(channel);
		}
		let mut threads: HashMap<Snowflake, Vec<&Channel>> = HashMap::new();
		for thread in guild.threads.values().filter(|thread| {
			thread
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
		let viewable = context.me.is_none_or(|me| {
			compute_permissions(guild, Some(channel), me, context.now).contains(Permissions::VIEW_CHANNEL)
		});
		let status = self.channel_status(channel, context.settings, viewable, context.now);
		let mut voice_members: Vec<Snowflake> = if is_voice(channel.kind) {
			guild
				.voice_states
				.values()
				.filter(|voice| voice.channel_id == Some(channel.id))
				.map(|voice| voice.user_id)
				.collect()
		} else {
			Vec::new()
		};
		voice_members.sort_unstable();
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
			voice_members,
		}
	}

	pub fn dm_rows(&self) -> Vec<DmRow> {
		let now = Utc::now();
		let settings = self.global_settings.as_ref();
		let mut rows: Vec<DmRow> = self
			.private_channels
			.values()
			.map(|channel| {
				let status = self.channel_status(channel, settings, true, now);
				DmRow {
					channel_id: channel.id,
					name: self.dm_name(channel),
					icon: channel.icon.clone().or_else(|| {
						let first = channel.recipient_ids.first()?;
						self.users.get(first)?.avatar.clone()
					}),
					recipients: channel.recipient_ids.clone(),
					unread: status.unread,
					mentions: status.mentions,
					last_message_id: channel.last_message_id,
				}
			})
			.collect();
		rows.sort_by_key(|row| Reverse((row.last_message_id, row.channel_id)));
		rows
	}

	fn dm_name(&self, channel: &Channel) -> String {
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
		let Some(cache) = self.messages.get(&channel_id) else {
			return vec![MessageRow::Loading];
		};
		let guild = self
			.channel_guild(channel_id)
			.and_then(|guild_id| self.guilds.get(&guild_id));
		let me = self.current_user_id();
		let last_read = self
			.read_states
			.get(&channel_id)
			.and_then(|state| state.last_message_id);
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
		let mut marker_placed = false;
		for (message, pending) in confirmed.chain(pending) {
			let day = local_day(message);
			if previous.is_none_or(|(_, previous_day)| previous_day != day) {
				rows.push(MessageRow::DaySeparator(day));
			}
			let is_unread = pending.is_none()
				&& last_read.is_some_and(|read| message.id > read)
				&& Some(message.author.id) != me;
			if !marker_placed && is_unread && (previous.is_some() || cache.reached_start) {
				rows.push(MessageRow::UnreadMarker);
				marker_placed = true;
			}
			let grouped = previous
				.is_some_and(|(earlier, earlier_day)| earlier_day == day && groups_with(earlier, message));
			rows.push(MessageRow::Message(
				self.message_view(guild, cache, message, grouped, pending),
			));
			previous = Some((message.as_ref(), day));
		}
		rows
	}

	fn message_view(
		&self,
		guild: Option<&GuildState>,
		cache: &ChannelMessages,
		message: &Arc<Message>,
		grouped_with_previous: bool,
		pending: Option<&PendingMessage>,
	) -> MessageView {
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
				cache.rendered.get(&message.id).cloned(),
				self.sent_by_plugin(message.id).map(str::to_owned),
				self.altered_by(message.id).map(str::to_owned),
			),
		};
		MessageView {
			message: Arc::clone(message),
			grouped_with_previous,
			pending: pending.is_some(),
			failed: pending.is_some_and(|pending| pending.failed),
			failure_reason: pending.and_then(|pending| pending.failure_reason.clone()),
			plugin,
			altered_by,
			rendered,
			author_color: over.and_then(|over| over.color).or(role_color),
			author_display: over.and_then(|over| over.name.clone()).unwrap_or(author_display),
			author_badge: over.and_then(|over| over.badge.clone()),
			member_roles,
		}
	}

	pub fn member_rows(&self, guild_id: Snowflake) -> Vec<MemberListRow> {
		let Some(guild) = self.guilds.get(&guild_id) else {
			return Vec::new();
		};
		let list = &guild.member_list;
		list.slots
			.iter()
			.filter_map(|slot| match slot {
				MemberSlot::Empty => None,
				MemberSlot::Group { id, count } => Some(MemberListRow::Group {
					id: id.clone(),
					name: group_name(guild, id),
					count: count_of(list.group_count(id).unwrap_or(*count)),
				}),
				MemberSlot::Member(user_id) => Some(MemberListRow::Member(self.member_row(guild, *user_id))),
			})
			.collect()
	}

	fn member_row(&self, guild: &GuildState, user_id: Snowflake) -> MemberRow {
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
