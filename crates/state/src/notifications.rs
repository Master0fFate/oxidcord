use std::sync::Arc;

use chrono::{DateTime, Utc};
use protocol::{
	Channel, ChannelType, DefaultMessageNotifications, Message, MuteConfig, NotificationLevel, Permissions,
	ReadState, Snowflake, StatusKind, ThreadMember, UserGuildSettings,
};

use crate::guild::GuildState;
use crate::permissions::compute_permissions;
use crate::state::State;

const ALERT_BACKLOG: usize = 16;

#[derive(Debug, Clone)]
pub struct Alert {
	pub id: u64,
	pub guild_id: Option<Snowflake>,
	pub message: Arc<Message>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ChannelStatus {
	pub unread: bool,
	pub mentions: u32,
	pub muted: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GuildStatus {
	pub unread: bool,
	pub mentions: u32,
	pub muted: bool,
}

pub(crate) fn mute_active(muted: bool, config: Option<&MuteConfig>, now: DateTime<Utc>) -> bool {
	muted
		&& config
			.and_then(|config| config.end_time)
			.is_none_or(|end| end > now)
}

pub(crate) fn guild_muted(settings: Option<&UserGuildSettings>, now: DateTime<Utc>) -> bool {
	settings.is_some_and(|settings| mute_active(settings.muted, settings.mute_config.as_ref(), now))
}

pub(crate) fn channel_muted(
	settings: Option<&UserGuildSettings>,
	channel: &Channel,
	now: DateTime<Utc>,
) -> bool {
	guild_muted(settings, now) || channel_override_muted(settings, channel, now)
}

fn channel_override_muted(
	settings: Option<&UserGuildSettings>,
	channel: &Channel,
	now: DateTime<Utc>,
) -> bool {
	if channel
		.member
		.as_ref()
		.is_some_and(|member| mute_active(member.muted, member.mute_config.as_ref(), now))
	{
		return true;
	}
	let Some(settings) = settings else {
		return false;
	};
	[Some(channel.id), channel.parent_id]
		.into_iter()
		.flatten()
		.any(|id| {
			settings
				.channel_overrides
				.iter()
				.any(|over| over.channel_id == id && mute_active(over.muted, over.mute_config.as_ref(), now))
		})
}

fn override_level(settings: Option<&UserGuildSettings>, channel_id: Snowflake) -> Option<NotificationLevel> {
	settings?
		.channel_overrides
		.iter()
		.find(|entry| entry.channel_id == channel_id)?
		.message_notifications
		.filter(|level| *level != NotificationLevel::Inherit)
}

fn mention_count(read_state: Option<&ReadState>) -> u32 {
	read_state
		.and_then(|state| state.mention_count)
		.map_or(0, |count| u32::try_from(count).unwrap_or(0))
}

impl State {
	pub fn own_status(&self) -> StatusKind {
		match &self.chosen_status {
			None | Some(StatusKind::Online) if self.afk => StatusKind::Idle,
			Some(status) => status.clone(),
			None => StatusKind::Online,
		}
	}

	pub fn is_afk(&self) -> bool {
		self.afk
	}

	pub fn alerts_after(&self, seen: u64) -> impl Iterator<Item = &Alert> {
		self.alerts.iter().filter(move |alert| alert.id > seen)
	}

	pub fn last_alert_id(&self) -> u64 {
		self.alert_count
	}

	pub fn thread_muted(&self, thread_id: Snowflake) -> bool {
		self.channel(thread_id)
			.and_then(|thread| thread.member.as_ref())
			.is_some_and(|member| mute_active(member.muted, member.mute_config.as_ref(), Utc::now()))
	}

	pub fn notification_level(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
	) -> NotificationLevel {
		let Some(guild_id) = guild_id else {
			return NotificationLevel::AllMessages;
		};
		let settings = self.guild_settings.get(&guild_id);
		let channel = channel_id.and_then(|id| self.channel(id));
		let followed = channel
			.and_then(|channel| channel.member.as_ref())
			.map(ThreadMember::notification_level)
			.filter(|level| *level != NotificationLevel::Inherit);
		let overridden = followed.or_else(|| {
			channel
				.into_iter()
				.flat_map(|channel| [Some(channel.id), channel.parent_id])
				.flatten()
				.find_map(|id| override_level(settings, id))
		});
		let chosen = settings
			.and_then(|settings| settings.message_notifications)
			.filter(|level| *level != NotificationLevel::Inherit);
		overridden.or(chosen).unwrap_or_else(|| {
			match self
				.guilds
				.get(&guild_id)
				.and_then(|guild| guild.default_notifications)
			{
				Some(DefaultMessageNotifications::OnlyMentions) => NotificationLevel::OnlyMentions,
				_ => NotificationLevel::AllMessages,
			}
		})
	}

	pub(crate) fn restore_status(&mut self, status: &str) -> bool {
		let status = StatusKind::from(status);
		if status.is_unknown()
			|| status == StatusKind::Offline
			|| self.chosen_status.as_ref() == Some(&status)
		{
			return false;
		}
		self.chosen_status = Some(status);
		true
	}

	pub(crate) fn wants_alert(&self, message: &Message, guild_id: Option<Snowflake>) -> bool {
		let Some(me) = self.current_user_id() else {
			return false;
		};
		if message.author.id == me {
			return false;
		}
		let settings = self.guild_settings(guild_id);
		let now = Utc::now();
		let muted = match self.channel(message.channel_id) {
			Some(channel) => channel_muted(settings, channel, now),
			None => guild_muted(settings, now),
		};
		if muted {
			return false;
		}
		match self.notification_level(guild_id, Some(message.channel_id)) {
			NotificationLevel::AllMessages => true,
			NotificationLevel::NoMessages => false,
			_ => self.mentions_me(message, guild_id),
		}
	}

	pub(crate) fn push_alert(&mut self, guild_id: Option<Snowflake>, message: Arc<Message>) {
		if self.alerts.len() == ALERT_BACKLOG {
			self.alerts.pop_front();
		}
		self.alert_count += 1;
		self.alerts.push_back(Alert {
			id: self.alert_count,
			guild_id,
			message,
		});
	}

	pub(crate) fn channel_status(
		&self,
		channel: &Channel,
		settings: Option<&UserGuildSettings>,
		viewable: bool,
		now: DateTime<Utc>,
	) -> ChannelStatus {
		let muted = channel_override_muted(settings, channel, now);
		if !viewable || channel.kind == ChannelType::GuildCategory {
			return ChannelStatus {
				muted,
				..ChannelStatus::default()
			};
		}
		let read_state = self.read_states.get(&channel.id);
		let behind = match (read_state, channel.last_message_id) {
			(Some(state), Some(last)) => last > state.last_message_id.unwrap_or_default(),
			_ => false,
		};
		ChannelStatus {
			unread: behind && !muted,
			mentions: mention_count(read_state),
			muted,
		}
	}

	pub(crate) fn guild_status(&self, guild: &GuildState, now: DateTime<Utc>) -> GuildStatus {
		let settings = self.guild_settings.get(&guild.id);
		let mut status = GuildStatus {
			muted: guild_muted(settings, now),
			..GuildStatus::default()
		};
		let Some(me) = self.current_user_id() else {
			return status;
		};
		for channel in guild.channels.values() {
			if channel.kind == ChannelType::GuildCategory {
				continue;
			}
			let viewable =
				compute_permissions(guild, Some(channel), me, now).contains(Permissions::VIEW_CHANNEL);
			let channel_status = self.channel_status(channel, settings, viewable, now);
			status.unread |= channel_status.unread;
			status.mentions += channel_status.mentions;
		}
		status
	}

	pub(crate) fn mentions_me(&self, message: &Message, guild_id: Option<Snowflake>) -> bool {
		let Some(me) = self.current_user_id() else {
			return false;
		};
		if message.mentions.iter().any(|user| user.id == me) {
			return true;
		}
		let settings = guild_id.and_then(|id| self.guild_settings.get(&id));
		if message.mention_everyone && !settings.is_some_and(|settings| settings.suppress_everyone) {
			return true;
		}
		if message.mention_roles.is_empty() || settings.is_some_and(|settings| settings.suppress_roles) {
			return false;
		}
		guild_id
			.and_then(|id| self.guilds.get(&id))
			.and_then(|guild| guild.members.get(&me))
			.is_some_and(|member| {
				member
					.roles
					.iter()
					.any(|role| message.mention_roles.contains(role))
			})
	}

	pub(crate) fn set_read_state(&mut self, channel_id: Snowflake, message_id: Snowflake, mentions: u32) {
		let state = self.read_states.entry(channel_id).or_insert_with(|| ReadState {
			id: channel_id,
			..ReadState::default()
		});
		state.last_message_id = Some(message_id);
		state.mention_count = Some(i64::from(mentions));
	}

	pub(crate) fn bump_mentions(&mut self, channel_id: Snowflake) {
		let state = self.read_states.entry(channel_id).or_insert_with(|| ReadState {
			id: channel_id,
			..ReadState::default()
		});
		state.mention_count = Some(state.mention_count.unwrap_or(0) + 1);
	}
}
