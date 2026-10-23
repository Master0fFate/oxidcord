use std::collections::HashMap;

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use protocol::{Channel, ChannelType, Emoji, Guild, Member, Role, Snowflake, VoiceState};

use crate::member_list::MemberList;

#[derive(Debug, Default)]
pub struct GuildState {
	pub id: Snowflake,
	pub name: String,
	pub icon: Option<String>,
	pub banner: Option<String>,
	pub owner_id: Option<Snowflake>,
	pub features: Vec<String>,
	pub member_count: Option<i64>,
	pub description: Option<String>,
	pub joined_at: Option<DateTime<Utc>>,
	pub large: bool,
	pub unavailable: bool,
	pub channels: IndexMap<Snowflake, Channel>,
	pub threads: IndexMap<Snowflake, Channel>,
	pub roles: IndexMap<Snowflake, Role>,
	pub members: HashMap<Snowflake, Member>,
	pub emojis: Vec<Emoji>,
	pub voice_states: HashMap<Snowflake, VoiceState>,
	pub member_list: MemberList,
}

pub fn is_thread(kind: ChannelType) -> bool {
	matches!(
		kind,
		ChannelType::AnnouncementThread | ChannelType::PublicThread | ChannelType::PrivateThread
	)
}

pub fn is_voice(kind: ChannelType) -> bool {
	matches!(kind, ChannelType::GuildVoice | ChannelType::GuildStageVoice)
}

pub fn stripped_member(member: &Member) -> Member {
	Member {
		user: None,
		nick: member.nick.clone(),
		avatar: member.avatar.clone(),
		banner: member.banner.clone(),
		roles: member.roles.clone(),
		joined_at: member.joined_at,
		premium_since: member.premium_since,
		deaf: member.deaf,
		mute: member.mute,
		flags: member.flags,
		pending: member.pending,
		permissions: member.permissions,
		communication_disabled_until: member.communication_disabled_until,
		avatar_decoration_data: member.avatar_decoration_data.clone(),
		guild_id: member.guild_id,
	}
}

impl GuildState {
	pub fn from_guild(guild: &Guild) -> Self {
		let mut state = Self {
			id: guild.id,
			..Self::default()
		};
		state.set_info(guild);
		state.set_roles_and_emojis(guild);
		state
	}

	pub fn set_info(&mut self, guild: &Guild) {
		if let Some(name) = &guild.name {
			self.name.clone_from(name);
		}
		self.icon.clone_from(&guild.icon);
		self.banner.clone_from(&guild.banner);
		self.description.clone_from(&guild.description);
		self.owner_id = guild.owner_id.or(self.owner_id);
		self.member_count = guild.member_count.or(self.member_count);
		self.joined_at = guild.joined_at.or(self.joined_at);
		self.large = guild.large.unwrap_or(self.large);
		self.unavailable = guild.unavailable.unwrap_or(false);
		if !guild.features.is_empty() {
			self.features.clone_from(&guild.features);
		}
	}

	pub fn set_roles_and_emojis(&mut self, guild: &Guild) {
		if !guild.roles.is_empty() {
			self.roles = guild.roles.iter().map(|role| (role.id, role.clone())).collect();
		}
		if !guild.emojis.is_empty() {
			self.emojis = guild.emojis.to_vec();
		}
	}

	pub fn channel(&self, id: Snowflake) -> Option<&Channel> {
		self.channels.get(&id).or_else(|| self.threads.get(&id))
	}

	pub fn channel_mut(&mut self, id: Snowflake) -> Option<&mut Channel> {
		match self.channels.get_mut(&id) {
			Some(channel) => Some(channel),
			None => self.threads.get_mut(&id),
		}
	}

	pub fn insert_channel(&mut self, channel: Channel) {
		if is_thread(channel.kind) {
			self.threads.insert(channel.id, channel);
		} else {
			self.channels.insert(channel.id, channel);
		}
	}

	pub fn remove_channel(&mut self, id: Snowflake) -> Option<Channel> {
		self.channels
			.shift_remove(&id)
			.or_else(|| self.threads.shift_remove(&id))
	}
}
