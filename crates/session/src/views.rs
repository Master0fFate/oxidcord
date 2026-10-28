use chrono::Utc;
use plugin_host::api::MessageRef;
use plugin_host::api::model::{
	ActivityView, ChannelDetail, Connection, Folder, GuildDetail, MemberInfo, MessageMeta, Presence,
	ReadState, Relationship, RelationshipKind, RoleInfo, UserProfile, VoiceDetail,
};
use plugin_host::convert::{emoji_ref, user_view};
use protocol::{Message, Snowflake, StatusKind};
use state::{ChannelMessages, ChannelUnread, RailEntry, State};

use crate::broker::voice_info;

fn millis(time: chrono::DateTime<Utc>) -> i64 {
	time.timestamp_millis()
}

fn non_empty(text: &Option<String>) -> Option<String> {
	text.as_ref().filter(|text| !text.is_empty()).cloned()
}

fn count(value: i64) -> u32 {
	u32::try_from(value).unwrap_or(0)
}

pub(crate) fn relationship(state: &State, relationship: &protocol::Relationship) -> Option<Relationship> {
	let kind = match relationship.kind {
		protocol::RelationshipKind::Friend => RelationshipKind::Friend,
		protocol::RelationshipKind::Blocked => RelationshipKind::Blocked,
		protocol::RelationshipKind::IncomingRequest => RelationshipKind::PendingIncoming,
		protocol::RelationshipKind::OutgoingRequest => RelationshipKind::PendingOutgoing,
		protocol::RelationshipKind::Implicit => RelationshipKind::Implicit,
		_ => return None,
	};
	let user = match state.user(relationship.id) {
		Some(user) => user_view(user),
		None => user_view(relationship.user.as_ref()?),
	};
	Some(Relationship {
		user,
		kind,
		since_ms: relationship.since.map(millis),
		nickname: non_empty(&relationship.nickname),
	})
}

pub(crate) fn implicit(state: &State, user_id: Snowflake) -> Option<Relationship> {
	Some(Relationship {
		user: user_view(state.user(user_id)?),
		kind: RelationshipKind::Implicit,
		since_ms: None,
		nickname: None,
	})
}

fn activity(activity: &protocol::Activity) -> ActivityView {
	ActivityView {
		kind: u8::try_from(i64::from(activity.kind)).unwrap_or(u8::MAX),
		name: activity.name.clone(),
		details: non_empty(&activity.details),
		state: non_empty(&activity.state),
		emoji: activity.emoji.as_ref().map(emoji_ref),
		start_ms: activity.timestamps.as_ref().and_then(|times| times.start),
		end_ms: activity.timestamps.as_ref().and_then(|times| times.end),
	}
}

pub(crate) fn presence(state: &State, user_id: Snowflake) -> Option<Presence> {
	Some(presence_view(user_id, state.presence(user_id)?))
}

pub(crate) fn presence_view(user_id: Snowflake, presence: &protocol::Presence) -> Presence {
	let platforms = presence
		.client_status
		.as_ref()
		.map(|status| {
			[
				("desktop", &status.desktop),
				("mobile", &status.mobile),
				("web", &status.web),
			]
			.into_iter()
			.filter(|(_, platform)| platform.as_ref().is_some_and(|kind| *kind != StatusKind::Offline))
			.map(|(name, _)| name.to_owned())
			.collect()
		})
		.unwrap_or_default();
	Presence {
		user_id: user_id.get(),
		status: presence.status.as_str().to_owned(),
		platforms,
		activities: presence.activities.iter().map(activity).collect(),
	}
}

pub(crate) fn read_state(unread: ChannelUnread) -> ReadState {
	ReadState {
		channel_id: unread.channel_id.get(),
		last_message_id: unread.last_message_id.map(Snowflake::get),
		last_read_id: unread.last_read_id.map(Snowflake::get),
		mention_count: unread.mentions,
		unread: unread.unread,
		muted: unread.muted,
	}
}

fn role_color(role: &protocol::Role) -> u32 {
	let primary = if role.color != 0 {
		role.color
	} else {
		role.colors
			.as_ref()
			.and_then(|colors| colors.primary_color)
			.unwrap_or(0)
	};
	count(primary)
}

pub(crate) fn roles(state: &State, guild_id: Snowflake) -> Option<Vec<RoleInfo>> {
	let guild = state.guild(guild_id)?;
	Some(
		guild
			.roles
			.values()
			.map(|role| RoleInfo {
				id: role.id.get(),
				name: role.name.clone(),
				color: role_color(role),
				position: i32::try_from(role.position).unwrap_or(0),
				hoist: role.hoist,
				icon: role.icon.clone(),
				permissions: role.permissions.bits(),
				mentionable: role.mentionable,
			})
			.collect(),
	)
}

pub(crate) fn member_info(state: &State, guild_id: Snowflake, user_id: Snowflake) -> Option<MemberInfo> {
	let guild = state.guild(guild_id)?;
	let member = guild.members.get(&user_id)?;
	let user = match state.user(user_id) {
		Some(user) => user_view(user),
		None => user_view(member.user.as_ref()?),
	};
	let now = Utc::now();
	Some(MemberInfo {
		user,
		nick: non_empty(&member.nick),
		roles: member.roles.iter().map(|role| role.get()).collect(),
		top_role: member
			.roles
			.iter()
			.filter_map(|id| guild.roles.get(id))
			.max_by_key(|role| role.position)
			.map(|role| role.id.get()),
		color: state.role_color(guild_id, user_id),
		joined_ms: member.joined_at.map(millis),
		timeout_until_ms: member
			.communication_disabled_until
			.filter(|until| *until > now)
			.map(millis),
		premium_since_ms: member.premium_since.map(millis),
		pending: member.pending.unwrap_or(false),
	})
}

pub(crate) fn guild_detail(state: &State, guild_id: Snowflake) -> Option<GuildDetail> {
	let guild = state.guild(guild_id)?;
	Some(GuildDetail {
		id: guild_id.get(),
		name: guild.name.clone(),
		icon: guild.icon.clone(),
		banner: guild.banner.clone(),
		description: non_empty(&guild.description),
		owner_id: guild.owner_id.map_or(0, Snowflake::get),
		member_count: guild.member_count.map(count),
		presence_count: None,
		boost_tier: guild.premium_tier,
		boost_count: guild.premium_subscription_count,
		created_ms: millis(guild_id.timestamp()),
		features: guild.features.clone(),
		vanity: non_empty(&guild.vanity_url_code),
	})
}

pub(crate) fn channel_detail(state: &State, channel_id: Snowflake, private: bool) -> Option<ChannelDetail> {
	let channel = state.channel(channel_id)?;
	let guild_id = state.channel_guild(channel_id);
	if guild_id.is_none() && !private {
		return None;
	}
	let name = match guild_id {
		Some(_) => channel.name.clone().unwrap_or_default(),
		None => state.dm_name(channel),
	};
	let mut recipients: Vec<u64> = channel.recipient_ids.iter().map(|id| id.get()).collect();
	for user in channel.recipients.iter() {
		if !recipients.contains(&user.id.get()) {
			recipients.push(user.id.get());
		}
	}
	Some(ChannelDetail {
		id: channel_id.get(),
		guild_id: guild_id.map(Snowflake::get),
		name,
		kind: u32::try_from(i64::from(channel.kind)).unwrap_or(u32::MAX),
		topic: non_empty(&channel.topic),
		nsfw: channel.nsfw.unwrap_or(false),
		parent_id: channel.parent_id.map(Snowflake::get),
		position: channel.position.map_or(0, count),
		recipients,
		owner_id: channel.owner_id.map(Snowflake::get),
		last_message_id: channel.last_message_id.map(Snowflake::get),
		rate_limit: channel.rate_limit_per_user.map_or(0, count),
		created_ms: millis(channel_id.timestamp()),
	})
}

pub(crate) fn folders(state: &State) -> Vec<Folder> {
	state
		.guild_layout()
		.into_iter()
		.map(|entry| match entry {
			RailEntry::Guild(id) => Folder {
				id: None,
				name: None,
				color: None,
				guilds: vec![id.get()],
			},
			RailEntry::Folder {
				id,
				name,
				color,
				guilds,
			} => Folder {
				id: Some(id),
				name: name.filter(|name| !name.is_empty()),
				color,
				guilds: guilds.into_iter().map(Snowflake::get).collect(),
			},
		})
		.collect()
}

pub(crate) fn user_profile(profile: &state::UserProfile) -> UserProfile {
	UserProfile {
		user: user_view(&profile.user),
		bio: profile.bio.clone(),
		pronouns: profile.pronouns.clone(),
		banner: profile.banner.clone(),
		accent_color: profile.accent_color.and_then(|color| u32::try_from(color).ok()),
		connections: profile
			.connected_accounts
			.iter()
			.map(|account| Connection {
				kind: account.kind.clone(),
				name: account.name.clone(),
				id: String::new(),
				verified: account.verified,
				url: None,
			})
			.collect(),
		premium_since_ms: profile.premium_since.map(millis),
		guild_nick: profile
			.guild_member
			.as_ref()
			.and_then(|member| non_empty(&member.nick)),
		mutual_guilds: profile.mutual_guilds.iter().map(|id| id.get()).collect(),
	}
}

pub(crate) fn message_meta(state: &State, cache: Option<&ChannelMessages>, message: &Message) -> MessageMeta {
	let guild_id = message
		.guild_id
		.or_else(|| state.channel_guild(message.channel_id));
	MessageMeta {
		guild_id: guild_id.map(Snowflake::get),
		reply_to: message
			.message_reference
			.as_ref()
			.filter(|_| !message.is_forward())
			.and_then(|reference| {
				Some(MessageRef {
					id: reference.message_id?.get(),
					channel_id: reference.channel_id.unwrap_or(message.channel_id).get(),
					guild_id: reference.guild_id.or(guild_id).map(Snowflake::get),
				})
			}),
		flags: message.flags.bits(),
		latency_ms: cache.and_then(|cache| cache.latency_ms(message.id)),
		author_color: guild_id.and_then(|guild| state.role_color(guild, message.author.id)),
		pinned: message.pinned,
		edited_ms: message.edited_timestamp.map(millis),
		nsfw_channel: state
			.channel(message.channel_id)
			.and_then(|channel| channel.nsfw)
			.unwrap_or(false),
	}
}

pub(crate) fn voice_detail(state: &State) -> Option<VoiceDetail> {
	let base = voice_info(state)?;
	let session = state.voice()?;
	let channel_name = state
		.channel(session.channel_id)
		.map(|channel| match session.guild_id {
			Some(_) => channel.name.clone().unwrap_or_default(),
			None => state.dm_name(channel),
		})
		.unwrap_or_default();
	Some(VoiceDetail {
		base,
		started_ms: session.connected_at.map_or(0, millis),
		channel_name,
		guild_name: session
			.guild_id
			.and_then(|guild_id| state.guild(guild_id))
			.map(|guild| guild.name.clone()),
		self_video: session.camera_on || session.screen_share.is_some(),
	})
}
