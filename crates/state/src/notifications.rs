use chrono::{DateTime, Utc};
use protocol::{
	Channel, ChannelType, Message, MuteConfig, Permissions, ReadState, Snowflake, UserGuildSettings,
};

use crate::guild::GuildState;
use crate::permissions::compute_permissions;
use crate::state::State;

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
	let Some(settings) = settings else {
		return false;
	};
	if mute_active(settings.muted, settings.mute_config.as_ref(), now) {
		return true;
	}
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

fn mention_count(read_state: Option<&ReadState>) -> u32 {
	read_state
		.and_then(|state| state.mention_count)
		.map_or(0, |count| u32::try_from(count).unwrap_or(0))
}

impl State {
	pub(crate) fn channel_status(
		&self,
		channel: &Channel,
		settings: Option<&UserGuildSettings>,
		viewable: bool,
		now: DateTime<Utc>,
	) -> ChannelStatus {
		let muted = channel_muted(settings, channel, now);
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
