use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use protocol::{
	Activity, ActivityAssets, ActivityKind, ActivityTimestamps, Channel, ChannelType, ClientStatus,
	DefaultMessageNotifications, DefaultReaction, Emoji, ExplicitContentFilter, ForumLayout, ForumSortOrder,
	ForumTag, GuildProperties, LenientVec, Member, NsfwLevel, PartialEmoji, PermissionOverwrite,
	PermissionOverwriteKind, Permissions, PremiumTier, Presence, Role, RoleColors, Snowflake, StatusKind,
	ThreadMetadata, User, VerificationLevel, VoiceState,
};
use rand::Rng;
use rand::rngs::StdRng;

use super::messages::{self, ChannelSpec};
use super::{Builder, CURRENT_USER_ID, corpus};

const ADMIN_COLOR: i64 = 0xE74C3C;
const MODS_COLOR: i64 = 0x2ECC71;
const MEMBERS_COLOR: i64 = 0x6666FF;
const MUTED_COLOR: i64 = 0x607D8B;
const THREAD_MESSAGE_CAP: usize = 40;
const FORUM_POST_MESSAGE_CAP: usize = 15;
const FIXED_NON_TEXT_CHANNELS: usize = 5;

pub struct GuildFixture {
	pub id: u64,
	pub properties: GuildProperties,
	pub roles: Vec<Role>,
	pub channel_ids: Vec<u64>,
	pub message_channel_ids: Vec<u64>,
	pub thread_ids: Vec<u64>,
	pub muted_channel_id: u64,
	pub emojis: Vec<Emoji>,
	pub members: Vec<Member>,
	pub member_index: HashMap<u64, usize>,
	pub presences: Vec<Presence>,
	pub voice_states: Vec<VoiceState>,
	pub joined_at: DateTime<Utc>,
}

impl GuildFixture {
	pub fn member(&self, user_id: u64) -> Option<&Member> {
		self.member_index.get(&user_id).map(|&index| &self.members[index])
	}

	pub fn current_member(&self) -> &Member {
		&self.members[0]
	}

	pub fn permissions_for(&self, member: &Member) -> Permissions {
		let granted = self
			.roles
			.iter()
			.filter(|role| role.id.0 == self.id || member.roles.contains(&role.id))
			.fold(Permissions::empty(), |acc, role| acc | role.permissions);
		if granted.contains(Permissions::ADMINISTRATOR) {
			Permissions::all()
		} else {
			granted
		}
	}
}

struct RoleIds {
	everyone: u64,
	admin: u64,
	mods: u64,
	members: u64,
	muted: u64,
}

fn base_permissions() -> Permissions {
	Permissions::VIEW_CHANNEL
		| Permissions::SEND_MESSAGES
		| Permissions::READ_MESSAGE_HISTORY
		| Permissions::ADD_REACTIONS
		| Permissions::EMBED_LINKS
		| Permissions::ATTACH_FILES
		| Permissions::USE_EXTERNAL_EMOJIS
		| Permissions::CONNECT
		| Permissions::SPEAK
		| Permissions::USE_VAD
		| Permissions::CHANGE_NICKNAME
		| Permissions::CREATE_INSTANT_INVITE
		| Permissions::CREATE_PUBLIC_THREADS
		| Permissions::SEND_MESSAGES_IN_THREADS
		| Permissions::USE_APPLICATION_COMMANDS
		| Permissions::USE_EXTERNAL_APPS
}

fn role(id: u64, name: &str, color: i64, hoist: bool, position: i64, permissions: Permissions) -> Role {
	Role {
		id: Snowflake(id),
		name: name.to_owned(),
		color,
		colors: Some(RoleColors {
			primary_color: Some(color),
			secondary_color: None,
			tertiary_color: None,
		}),
		hoist,
		position,
		permissions,
		mentionable: hoist,
		flags: Some(0),
		..Role::default()
	}
}

fn build_roles(ids: &RoleIds) -> Vec<Role> {
	let mut everyone = role(ids.everyone, "@everyone", 0, false, 0, base_permissions());
	everyone.mentionable = false;
	vec![
		everyone,
		role(ids.muted, "Muted", MUTED_COLOR, false, 1, Permissions::empty()),
		role(
			ids.members,
			"Members",
			MEMBERS_COLOR,
			false,
			2,
			Permissions::EMBED_LINKS | Permissions::ATTACH_FILES,
		),
		role(
			ids.mods,
			"Mods",
			MODS_COLOR,
			true,
			3,
			Permissions::MANAGE_MESSAGES
				| Permissions::KICK_MEMBERS
				| Permissions::BAN_MEMBERS
				| Permissions::MODERATE_MEMBERS
				| Permissions::MANAGE_THREADS,
		),
		role(
			ids.admin,
			"Admin",
			ADMIN_COLOR,
			true,
			4,
			Permissions::ADMINISTRATOR,
		),
	]
}

fn guild_channel(
	id: u64,
	guild_id: u64,
	kind: ChannelType,
	name: &str,
	position: i64,
	parent: Option<u64>,
) -> Channel {
	Channel {
		id: Snowflake(id),
		kind,
		guild_id: Some(Snowflake(guild_id)),
		position: Some(position),
		name: Some(name.to_owned()),
		parent_id: parent.map(Snowflake),
		nsfw: Some(false),
		..Channel::default()
	}
}

fn muted_overwrite(muted_role: u64) -> PermissionOverwrite {
	PermissionOverwrite {
		id: Snowflake(muted_role),
		kind: PermissionOverwriteKind::Role,
		allow: Permissions::empty(),
		deny: Permissions::SEND_MESSAGES | Permissions::ADD_REACTIONS | Permissions::SEND_MESSAGES_IN_THREADS,
	}
}

fn text_channel_name(index: usize) -> String {
	let names = corpus::TEXT_CHANNEL_NAMES;
	if index < names.len() {
		names[index].to_owned()
	} else {
		format!("{}-{}", names[index % names.len()], index / names.len() + 1)
	}
}

fn presence_for(rng: &mut StdRng, user_id: u64, status: StatusKind, variant: usize) -> Presence {
	let activities = if status == StatusKind::Offline {
		Vec::new()
	} else {
		match variant % 4 {
			0 => vec![playing(rng)],
			1 => vec![listening(rng)],
			2 => vec![custom_status(rng)],
			_ => Vec::new(),
		}
	};
	let platform = (status != StatusKind::Offline).then(|| status.clone());
	Presence {
		user: None,
		user_id: Some(Snowflake(user_id)),
		guild_id: None,
		status,
		activities: LenientVec::from(activities),
		client_status: platform.map(|status| ClientStatus {
			desktop: Some(status),
			mobile: None,
			web: None,
		}),
	}
}

pub fn random_presence(rng: &mut StdRng, user_id: u64, status: StatusKind) -> Presence {
	let variant = rng.random_range(0..4);
	presence_for(rng, user_id, status, variant)
}

fn now_ms() -> i64 {
	Utc::now().timestamp_millis()
}

fn playing(rng: &mut StdRng) -> Activity {
	Activity {
		id: Some(corpus::hex_hash(rng)),
		name: (*corpus::pick(rng, corpus::GAMES)).to_owned(),
		kind: ActivityKind::Playing,
		created_at: Some(now_ms()),
		timestamps: Some(ActivityTimestamps {
			start: Some(now_ms() - 3_600_000),
			end: None,
		}),
		application_id: Some(Snowflake(rng.random_range(10u64.pow(17)..10u64.pow(18)))),
		details: Some("In a session".to_owned()),
		..Activity::default()
	}
}

fn listening(rng: &mut StdRng) -> Activity {
	let (title, artist, album) = *corpus::pick(rng, corpus::SONGS);
	Activity {
		id: Some("spotify:1".to_owned()),
		name: "Spotify".to_owned(),
		kind: ActivityKind::Listening,
		created_at: Some(now_ms()),
		timestamps: Some(ActivityTimestamps {
			start: Some(now_ms() - 60_000),
			end: Some(now_ms() + 180_000),
		}),
		details: Some(title.to_owned()),
		state: Some(artist.to_owned()),
		assets: Some(ActivityAssets {
			large_image: Some("spotify:ab67616d0000b273mock".to_owned()),
			large_text: Some(album.to_owned()),
			..ActivityAssets::default()
		}),
		..Activity::default()
	}
}

fn custom_status(rng: &mut StdRng) -> Activity {
	let (text, emoji) = *corpus::pick(rng, corpus::CUSTOM_STATUSES);
	Activity {
		id: Some("custom".to_owned()),
		name: "Custom Status".to_owned(),
		kind: ActivityKind::Custom,
		created_at: Some(now_ms()),
		state: Some(text.to_owned()),
		emoji: Some(PartialEmoji {
			id: None,
			name: Some(emoji.to_owned()),
			animated: false,
		}),
		..Activity::default()
	}
}

struct Channels {
	all: Vec<u64>,
	message: Vec<u64>,
	text: Vec<u64>,
	forum: u64,
	voice: Vec<u64>,
	system: u64,
	muted: u64,
}

fn add_channel(builder: &mut Builder, list: &mut Vec<u64>, channel: Channel) {
	list.push(channel.id.0);
	builder.channels.insert(channel.id.0, channel);
}

fn build_channels(builder: &mut Builder, guild_id: u64, roles: &RoleIds) -> Channels {
	let text_count = builder
		.config
		.channels_per_guild
		.saturating_sub(FIXED_NON_TEXT_CHANNELS)
		.max(2);
	let mut position = -1;
	let mut next_position = || {
		position += 1;
		position
	};
	let mut layout = Channels {
		all: Vec::new(),
		message: Vec::new(),
		text: Vec::new(),
		forum: 0,
		voice: Vec::new(),
		system: 0,
		muted: 0,
	};
	let text_category = builder.ids.next();
	let voice_category = builder.ids.next();
	let category = guild_channel(
		text_category,
		guild_id,
		ChannelType::GuildCategory,
		"Text Channels",
		next_position(),
		None,
	);
	add_channel(builder, &mut layout.all, category);
	for index in 0..text_count {
		let id = builder.ids.next();
		let private = index == text_count - 1;
		let name = if private {
			"staff-only".to_owned()
		} else {
			text_channel_name(index)
		};
		let mut channel = guild_channel(
			id,
			guild_id,
			ChannelType::GuildText,
			&name,
			next_position(),
			Some(text_category),
		);
		channel.topic = Some(format!("talk about {name} here"));
		channel.rate_limit_per_user = Some(0);
		let mut overwrites = vec![muted_overwrite(roles.muted)];
		if private {
			overwrites.push(PermissionOverwrite {
				id: Snowflake(roles.everyone),
				kind: PermissionOverwriteKind::Role,
				allow: Permissions::empty(),
				deny: Permissions::VIEW_CHANNEL,
			});
			overwrites.push(PermissionOverwrite {
				id: Snowflake(roles.admin),
				kind: PermissionOverwriteKind::Role,
				allow: Permissions::VIEW_CHANNEL,
				deny: Permissions::empty(),
			});
		}
		channel.permission_overwrites = LenientVec::from(overwrites);
		add_channel(builder, &mut layout.all, channel);
		layout.message.push(id);
		layout.text.push(id);
	}
	layout.system = layout.text[0];
	layout.muted = if text_count >= 3 {
		layout.text[1]
	} else {
		layout.text[0]
	};

	let announcement_id = builder.ids.next();
	let mut announcement = guild_channel(
		announcement_id,
		guild_id,
		ChannelType::GuildAnnouncement,
		"announcements",
		next_position(),
		Some(text_category),
	);
	announcement.topic = Some("news and releases".to_owned());
	announcement.permission_overwrites = LenientVec::from(vec![muted_overwrite(roles.muted)]);
	add_channel(builder, &mut layout.all, announcement);
	layout.message.push(announcement_id);

	layout.forum = builder.ids.next();
	let mut forum = guild_channel(
		layout.forum,
		guild_id,
		ChannelType::GuildForum,
		"help-forum",
		next_position(),
		Some(text_category),
	);
	forum.topic = Some("ask questions, one post per topic".to_owned());
	forum.available_tags = LenientVec::from(vec![
		forum_tag(builder.ids.next(), "bug", "🐛", false),
		forum_tag(builder.ids.next(), "question", "❓", false),
		forum_tag(builder.ids.next(), "solved", "✅", true),
	]);
	forum.default_reaction_emoji = Some(DefaultReaction {
		emoji_id: None,
		emoji_name: Some("👍".to_owned()),
	});
	forum.default_sort_order = Some(ForumSortOrder::LatestActivity);
	forum.default_forum_layout = Some(ForumLayout::ListView);
	add_channel(builder, &mut layout.all, forum);

	let category = guild_channel(
		voice_category,
		guild_id,
		ChannelType::GuildCategory,
		"Voice Channels",
		next_position(),
		None,
	);
	add_channel(builder, &mut layout.all, category);
	for (name, limit) in [("Lounge", 0), ("Gaming", 5)] {
		let id = builder.ids.next();
		let mut voice = guild_channel(
			id,
			guild_id,
			ChannelType::GuildVoice,
			name,
			next_position(),
			Some(voice_category),
		);
		voice.bitrate = Some(64_000);
		voice.user_limit = Some(limit);
		add_channel(builder, &mut layout.all, voice);
		layout.voice.push(id);
	}
	let stage_id = builder.ids.next();
	let mut stage = guild_channel(
		stage_id,
		guild_id,
		ChannelType::GuildStageVoice,
		"Town Hall",
		next_position(),
		Some(voice_category),
	);
	stage.bitrate = Some(64_000);
	add_channel(builder, &mut layout.all, stage);
	layout
}

fn forum_tag(id: u64, name: &str, emoji: &str, moderated: bool) -> ForumTag {
	ForumTag {
		id: Snowflake(id),
		name: name.to_owned(),
		moderated,
		emoji_id: None,
		emoji_name: Some(emoji.to_owned()),
	}
}

fn thread_channel(
	id: u64,
	guild_id: u64,
	parent: u64,
	name: &str,
	owner: u64,
	anchor: DateTime<Utc>,
) -> Channel {
	Channel {
		id: Snowflake(id),
		kind: ChannelType::PublicThread,
		guild_id: Some(Snowflake(guild_id)),
		parent_id: Some(Snowflake(parent)),
		name: Some(name.to_owned()),
		owner_id: Some(Snowflake(owner)),
		rate_limit_per_user: Some(0),
		member_count: Some(3),
		thread_metadata: Some(ThreadMetadata {
			archived: false,
			auto_archive_duration: Some(1440),
			archive_timestamp: Some(anchor - Duration::days(2)),
			locked: false,
			invitable: None,
			create_timestamp: Some(anchor - Duration::days(9)),
		}),
		..Channel::default()
	}
}

fn build_members(builder: &mut Builder, index: usize, guild_id: u64, roles: &RoleIds) -> Vec<Member> {
	let count = builder.config.members_per_guild.max(1);
	let pool_len = builder.people.pool.len();
	let start = index * (count / 2 + 1) % pool_len;
	let anchor = builder.anchor;
	let me_roles = match index % 3 {
		0 => vec![roles.admin, roles.members],
		1 => vec![roles.mods, roles.members],
		_ => vec![roles.members, roles.muted],
	};
	let timed_out_index = (count > 1).then(|| 3.min(count - 1));
	let mut members = Vec::with_capacity(count);
	members.push(Member {
		user: Some(builder.people.current.clone()),
		roles: me_roles.into_iter().map(Snowflake).collect(),
		joined_at: Some(anchor - Duration::days(400)),
		flags: Some(0),
		pending: Some(false),
		guild_id: Some(Snowflake(guild_id)),
		..Member::default()
	});
	for offset in 1..count {
		let user = builder.people.pool[(start + offset) % pool_len].clone();
		let mut member_roles = Vec::new();
		if offset % 10 == 1 {
			member_roles.push(roles.admin);
		} else if offset % 7 == 2 {
			member_roles.push(roles.mods);
		}
		member_roles.push(roles.members);
		if offset % 13 == 5 {
			member_roles.push(roles.muted);
		}
		let nick =
			(offset % 4 == 1).then(|| format!("{} [{offset}]", user.global_name.as_deref().unwrap_or("")));
		members.push(Member {
			nick,
			roles: member_roles.into_iter().map(Snowflake).collect(),
			joined_at: Some(anchor - Duration::days(builder.rng.random_range(30..800))),
			premium_since: (offset % 11 == 0).then(|| anchor - Duration::days(60)),
			flags: Some(0),
			pending: Some(false),
			communication_disabled_until: (timed_out_index == Some(offset))
				.then(|| anchor + Duration::days(3)),
			guild_id: Some(Snowflake(guild_id)),
			user: Some(user),
			..Member::default()
		});
	}
	members
}

fn build_emojis(builder: &mut Builder, owner: &User) -> Vec<Emoji> {
	[
		("pepe_ok", false, true),
		("blob_wave", false, true),
		("party_parrot", true, true),
		("retired_blob", false, false),
	]
	.into_iter()
	.map(|(name, animated, available)| Emoji {
		id: Some(Snowflake(builder.ids.next())),
		name: Some(name.to_owned()),
		roles: Vec::new(),
		user: Some(owner.clone()),
		require_colons: Some(true),
		managed: Some(false),
		animated: Some(animated),
		available: Some(available),
	})
	.collect()
}

fn build_presences(builder: &mut Builder, members: &[Member]) -> Vec<Presence> {
	let mut presences = Vec::new();
	for (index, member) in members.iter().enumerate() {
		let Some(user) = &member.user else {
			continue;
		};
		let status = if index == 0 {
			StatusKind::Online
		} else {
			match builder.rng.random_range(0..100) {
				0..40 => StatusKind::Online,
				40..55 => StatusKind::Idle,
				55..65 => StatusKind::Dnd,
				_ => StatusKind::Offline,
			}
		};
		if status == StatusKind::Offline {
			continue;
		}
		presences.push(presence_for(&mut builder.rng, user.id.0, status, index));
	}
	presences
}

fn build_voice_states(
	builder: &mut Builder,
	guild_id: u64,
	voice: &[u64],
	members: &[Member],
) -> Vec<VoiceState> {
	let candidates: Vec<&Member> = members
		.iter()
		.filter(|member| {
			member
				.user
				.as_ref()
				.is_some_and(|user| user.id.0 != CURRENT_USER_ID)
		})
		.collect();
	if candidates.is_empty() {
		return Vec::new();
	}
	let mut states = Vec::new();
	let mut cursor = 0;
	for &channel_id in voice {
		let occupants = 2 + builder.rng.random_range(0..2);
		for _ in 0..occupants {
			let member = candidates[cursor % candidates.len()];
			cursor += 1;
			states.push(VoiceState {
				guild_id: Some(Snowflake(guild_id)),
				channel_id: Some(Snowflake(channel_id)),
				user_id: member.user.as_ref().map(|user| user.id).unwrap_or_default(),
				member: Some(member.clone()),
				session_id: Some(corpus::hex_hash(&mut builder.rng)),
				deaf: false,
				mute: false,
				self_deaf: builder.rng.random_bool(0.2),
				self_mute: builder.rng.random_bool(0.4),
				self_stream: Some(false),
				self_video: false,
				suppress: false,
				request_to_speak_timestamp: None,
			});
		}
	}
	states
}

pub fn build(builder: &mut Builder, index: usize) -> GuildFixture {
	let guild_id = builder.ids.next();
	let role_ids = RoleIds {
		everyone: guild_id,
		admin: builder.ids.next(),
		mods: builder.ids.next(),
		members: builder.ids.next(),
		muted: builder.ids.next(),
	};
	let roles = build_roles(&role_ids);
	let members = build_members(builder, index, guild_id, &role_ids);
	let member_ids: Vec<u64> = members
		.iter()
		.filter_map(|member| member.user.as_ref().map(|u| u.id.0))
		.collect();
	let owner = members
		.get(1)
		.or(members.first())
		.and_then(|member| member.user.clone())
		.unwrap_or_default();
	let emojis = build_emojis(builder, &owner);
	let layout = build_channels(builder, guild_id, &role_ids);
	let anchor = builder.anchor;

	let first_text = layout.text[0];
	let thread_names = ["bug triage", "weekend plans"];
	let mut thread_ids = Vec::new();
	let mut chat_threads = Vec::new();
	for (offset, name) in thread_names.iter().enumerate() {
		let id = builder.ids.next();
		let author = member_ids[(offset + 1) % member_ids.len()];
		let thread = thread_channel(id, guild_id, first_text, name, author, anchor);
		chat_threads.push(thread.clone());
		builder.channels.insert(id, thread);
		thread_ids.push(id);
	}
	let forum_tags: Vec<Snowflake> = builder.channels[&layout.forum]
		.available_tags
		.iter()
		.map(|tag| tag.id)
		.collect();
	let mut forum_posts = Vec::new();
	for (offset, name) in ["crash on startup", "how do i theme the sidebar"]
		.iter()
		.enumerate()
	{
		let id = builder.ids.next();
		let author = member_ids[(offset + 2) % member_ids.len()];
		let mut post = thread_channel(id, guild_id, layout.forum, name, author, anchor);
		post.applied_tags = vec![forum_tags[offset % forum_tags.len()]];
		forum_posts.push(post.clone());
		builder.channels.insert(id, post);
		thread_ids.push(id);
	}

	let authors: Vec<&User> = members.iter().filter_map(|member| member.user.as_ref()).collect();
	let mention_targets: Vec<(u64, String)> = layout
		.text
		.iter()
		.map(|id| (*id, builder.channels[id].name.clone().unwrap_or_default()))
		.collect();
	let mut generated = Vec::new();
	let mut variant = index;
	let count = builder.config.messages_per_channel;
	for &channel_id in &layout.message {
		let threads = if channel_id == first_text {
			chat_threads.as_slice()
		} else {
			&[]
		};
		let spec = ChannelSpec {
			channel_id,
			guild_id: Some(guild_id),
			count,
			specials: true,
			variant,
			anchor,
			me: &builder.people.current,
			authors: &authors,
			roles: &roles,
			emojis: &emojis,
			channels: &mention_targets,
			threads,
		};
		generated.push((channel_id, messages::generate(&mut builder.rng, &spec)));
		variant += 1;
	}
	for thread in chat_threads.iter().chain(forum_posts.iter()) {
		let cap = if thread.parent_id.map(Snowflake::get) == Some(layout.forum) {
			FORUM_POST_MESSAGE_CAP
		} else {
			THREAD_MESSAGE_CAP
		};
		let spec = ChannelSpec {
			channel_id: thread.id.0,
			guild_id: Some(guild_id),
			count: count.min(cap),
			specials: false,
			variant,
			anchor,
			me: &builder.people.current,
			authors: &authors,
			roles: &roles,
			emojis: &emojis,
			channels: &[],
			threads: &[],
		};
		generated.push((thread.id.0, messages::generate(&mut builder.rng, &spec)));
		variant += 1;
	}
	for (channel_id, channel_messages) in generated {
		if let Some(channel) = builder.channels.get_mut(&channel_id) {
			channel.last_message_id = channel_messages.keys().next_back().copied().map(Snowflake);
			channel.message_count = Some(channel_messages.len() as i64);
			channel.total_message_sent = channel.message_count;
			channel.last_pin_timestamp = channel_messages
				.values()
				.filter(|message| message["pinned"].as_bool() == Some(true))
				.filter_map(|message| message["timestamp"].as_str())
				.filter_map(|text| DateTime::parse_from_rfc3339(text).ok())
				.map(|time| time.with_timezone(&Utc))
				.max();
		}
		builder.messages.insert(channel_id, channel_messages);
	}

	let presences = build_presences(builder, &members);
	let voice_states = build_voice_states(builder, guild_id, &layout.voice, &members);
	let member_index = members
		.iter()
		.enumerate()
		.filter_map(|(position, member)| member.user.as_ref().map(|user| (user.id.0, position)))
		.collect();
	let name = corpus::GUILD_NAMES[index % corpus::GUILD_NAMES.len()];
	let name = if index < corpus::GUILD_NAMES.len() {
		name.to_owned()
	} else {
		format!("{name} {}", index / corpus::GUILD_NAMES.len() + 1)
	};
	let icon = index
		.is_multiple_of(2)
		.then(|| corpus::hex_hash(&mut builder.rng));
	let properties = GuildProperties {
		id: Some(Snowflake(guild_id)),
		name: Some(name),
		icon,
		owner_id: Some(owner.id),
		afk_channel_id: layout.voice.first().copied().map(Snowflake),
		afk_timeout: Some(300),
		verification_level: Some(VerificationLevel::Low),
		default_message_notifications: Some(DefaultMessageNotifications::OnlyMentions),
		explicit_content_filter: Some(ExplicitContentFilter::Disabled),
		features: vec!["COMMUNITY".to_owned(), "NEWS".to_owned()],
		system_channel_id: Some(Snowflake(layout.system)),
		max_members: Some(500_000),
		description: Some("A mock guild for headless runs".to_owned()),
		premium_tier: Some(PremiumTier::Tier1),
		premium_subscription_count: Some(3),
		preferred_locale: Some("en-US".to_owned()),
		nsfw_level: Some(NsfwLevel::Default),
		..GuildProperties::default()
	};
	GuildFixture {
		id: guild_id,
		properties,
		roles,
		channel_ids: layout.all,
		message_channel_ids: layout.message,
		thread_ids,
		muted_channel_id: layout.muted,
		emojis,
		members,
		member_index,
		presences,
		voice_states,
		joined_at: anchor - Duration::days(400),
	}
}
