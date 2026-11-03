use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Utc};
use protocol::{
	Activity, ActivityAssets, ActivityKind, ActivityParty, ActivityTimestamps, Channel, ChannelFlags,
	ChannelType, ClientStatus, DefaultMessageNotifications, DefaultReaction, Emoji, ExplicitContentFilter,
	ForumLayout, ForumSortOrder, ForumTag, GuildProperties, LenientVec, Member, NsfwLevel, PartialEmoji,
	PermissionOverwrite, PermissionOverwriteKind, Permissions, PremiumTier, Presence, Role, RoleColors,
	ScheduledEvent, ScheduledEventKind, ScheduledEventMetadata, ScheduledEventStatus, Snowflake,
	StageInstance, StagePrivacy, StatusKind, SystemChannelFlags, ThreadMetadata, User, VerificationLevel,
	VideoQualityMode, VoiceState,
};
use rand::Rng;
use rand::rngs::StdRng;
use serde_json::Value;

use super::messages::{self, ChannelSpec};
use super::people;
use super::{Builder, CURRENT_USER_ID, corpus};

const ADMIN_COLOR: i64 = 0xE74C3C;
const MODS_COLOR: i64 = 0x2ECC71;
const MEMBERS_COLOR: i64 = 0x6666FF;
const MUTED_COLOR: i64 = 0x607D8B;
const THREAD_MESSAGE_CAP: usize = 40;
const FORUM_POST_MESSAGE_CAP: usize = 15;
const FIXED_NON_TEXT_CHANNELS: usize = 5;
const SLOWMODE_SECONDS: i64 = 10;
const SLOWMODE_CHANNEL_INDEX: usize = 1;
const LARGE_GUILD_STRIDE: usize = 3;
const LARGE_GUILD_MEMBERS: usize = 650;
const PIN_BACKLOG: usize = 60;
const INVITABLE_MESSAGES: usize = 30;
const INVITABLE_MEMBERS: usize = 12;
const INVITABLE_NAME: &str = "Rules Hall";
const SPARE_NAME: &str = "New Server";
pub const MEMBER_GATE_FEATURE: &str = "MEMBER_VERIFICATION_GATE_ENABLED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuildOrigin {
	Seeded,
	Invitable,
	Spare,
}

pub struct GuildFixture {
	pub id: u64,
	pub properties: GuildProperties,
	pub roles: Vec<Role>,
	pub channel_ids: Vec<u64>,
	pub message_channel_ids: Vec<u64>,
	pub thread_ids: Vec<u64>,
	pub private_thread_ids: Vec<u64>,
	pub muted_channel_id: u64,
	pub emojis: Vec<Emoji>,
	pub members: Vec<Member>,
	pub member_index: HashMap<u64, usize>,
	pub presences: Vec<Presence>,
	pub voice_states: Vec<VoiceState>,
	pub scheduled_events: Vec<ScheduledEvent>,
	pub stage_instance: Option<StageInstance>,
	pub joined_at: DateTime<Utc>,
	pub large: bool,
	pub origin: GuildOrigin,
}

impl GuildFixture {
	pub fn member(&self, user_id: u64) -> Option<&Member> {
		self.member_index.get(&user_id).map(|&index| &self.members[index])
	}

	pub fn current_member(&self) -> &Member {
		&self.members[0]
	}

	pub fn permissions_for(&self, member: &Member) -> Permissions {
		if member.id().is_some() && member.id() == self.properties.owner_id {
			return Permissions::all();
		}
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
		| Permissions::SET_VOICE_CHANNEL_STATUS
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

const MODS_GRADIENT: i64 = 0x00c2ff;
const ADMIN_ICON: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
const MODS_EMOJI: &str = "\u{1F6E1}\u{FE0F}";

fn gradient(mut role: Role, secondary: i64) -> Role {
	if let Some(colors) = role.colors.as_mut() {
		colors.secondary_color = Some(secondary);
	}
	role
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
		Role {
			unicode_emoji: Some(MODS_EMOJI.to_owned()),
			..gradient(
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
				MODS_GRADIENT,
			)
		},
		Role {
			icon: Some(ADMIN_ICON.to_owned()),
			..role(
				ids.admin,
				"Admin",
				ADMIN_COLOR,
				true,
				4,
				Permissions::ADMINISTRATOR,
			)
		},
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

fn role_overwrite(role: u64, allow: Permissions, deny: Permissions) -> PermissionOverwrite {
	PermissionOverwrite {
		id: Snowflake(role),
		kind: PermissionOverwriteKind::Role,
		allow,
		deny,
	}
}

fn muted_overwrite(muted_role: u64) -> PermissionOverwrite {
	role_overwrite(
		muted_role,
		Permissions::empty(),
		Permissions::SEND_MESSAGES | Permissions::ADD_REACTIONS | Permissions::SEND_MESSAGES_IN_THREADS,
	)
}

fn pin_backlog(messages: &mut BTreeMap<u64, Value>) {
	messages
		.values_mut()
		.filter(|message| message["type"].as_i64() == Some(0))
		.take(PIN_BACKLOG)
		.for_each(|message| message["pinned"] = Value::Bool(true));
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
	let id = corpus::hex_hash(rng);
	let request = if id.as_bytes()[0] % 2 == 0 {
		Activity::FLAG_JOIN_REQUEST
	} else {
		0
	};
	Activity {
		session_id: Some(format!("{id}-session")),
		party: Some(ActivityParty {
			id: Some(format!("{id}-party")),
			size: vec![2, 4],
		}),
		id: Some(id),
		name: (*corpus::pick(rng, corpus::GAMES)).to_owned(),
		kind: ActivityKind::Playing,
		created_at: Some(now_ms()),
		timestamps: Some(ActivityTimestamps {
			start: Some(now_ms() - 3_600_000),
			end: None,
		}),
		application_id: Some(Snowflake(rng.random_range(10u64.pow(17)..10u64.pow(18)))),
		details: Some("In a session".to_owned()),
		flags: Some(Activity::FLAG_INSTANCE | Activity::FLAG_JOIN | request),
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
	stage: u64,
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
		stage: 0,
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
		channel.rate_limit_per_user = Some(if index == SLOWMODE_CHANNEL_INDEX {
			SLOWMODE_SECONDS
		} else {
			0
		});
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
	announcement.flags = ChannelFlags::IS_SPOILER_CHANNEL;
	announcement.permission_overwrites = LenientVec::from(vec![
		muted_overwrite(roles.muted),
		role_overwrite(
			roles.members,
			Permissions::MANAGE_CHANNELS,
			Permissions::USE_EXTERNAL_EMOJIS | Permissions::USE_EXTERNAL_STICKERS,
		),
	]);
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

	let mut obfuscated = guild_channel(
		builder.ids.next(),
		guild_id,
		ChannelType::GuildText,
		"___hidden___",
		next_position(),
		Some(text_category),
	);
	obfuscated.flags = ChannelFlags::CHANNEL_OBFUSCATED;
	add_channel(builder, &mut layout.all, obfuscated);

	let category = guild_channel(
		voice_category,
		guild_id,
		ChannelType::GuildCategory,
		"Voice Channels",
		next_position(),
		None,
	);
	add_channel(builder, &mut layout.all, category);
	for (name, limit, bitrate, status) in [
		("Lounge", 0, 64_000, Some("movie night, bring snacks")),
		("Gaming", 2, 96_000, None),
	] {
		let id = builder.ids.next();
		let mut voice = guild_channel(
			id,
			guild_id,
			ChannelType::GuildVoice,
			name,
			next_position(),
			Some(voice_category),
		);
		voice.bitrate = Some(bitrate);
		voice.user_limit = Some(limit);
		voice.status = status.map(str::to_owned);
		voice.video_quality_mode = Some(VideoQualityMode::Auto);
		if limit == 0 {
			voice.permission_overwrites = LenientVec::from(vec![role_overwrite(
				roles.members,
				Permissions::empty(),
				Permissions::CONNECT,
			)]);
		}
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
	layout.stage = stage_id;
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

fn is_large(index: usize) -> bool {
	index % LARGE_GUILD_STRIDE == 1
}

fn build_members(builder: &mut Builder, index: usize, guild_id: u64, roles: &RoleIds) -> Vec<Member> {
	let count = if is_large(index) {
		builder.config.members_per_guild.max(LARGE_GUILD_MEMBERS)
	} else {
		builder.config.members_per_guild.max(1)
	};
	people::grow_pool(builder, count);
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

fn scheduled_event(
	builder: &mut Builder,
	guild_id: u64,
	creator: Snowflake,
	name: &str,
	description: &str,
	kind: ScheduledEventKind,
	status: ScheduledEventStatus,
) -> ScheduledEvent {
	ScheduledEvent {
		id: Snowflake(builder.ids.next()),
		guild_id: Snowflake(guild_id),
		creator_id: Some(creator),
		name: name.to_owned(),
		description: Some(description.to_owned()),
		status,
		entity_type: kind,
		..ScheduledEvent::default()
	}
}

fn build_scheduled_events(
	builder: &mut Builder,
	guild_id: u64,
	layout: &Channels,
	creator: Snowflake,
) -> Vec<ScheduledEvent> {
	let anchor = builder.anchor;
	let mut game_night = scheduled_event(
		builder,
		guild_id,
		creator,
		"Game night",
		"Bring your own snacks and join the lounge.",
		ScheduledEventKind::Voice,
		ScheduledEventStatus::Scheduled,
	);
	game_night.channel_id = layout.voice.first().copied().map(Snowflake);
	game_night.scheduled_start_time = Some(anchor + Duration::hours(1));
	game_night.scheduled_end_time = Some(anchor + Duration::hours(3));
	game_night.user_count = Some(4);
	let mut town_hall = scheduled_event(
		builder,
		guild_id,
		creator,
		"Town hall Q&A",
		"Open questions for the mods.",
		ScheduledEventKind::StageInstance,
		ScheduledEventStatus::Active,
	);
	town_hall.channel_id = Some(Snowflake(layout.stage));
	town_hall.scheduled_start_time = Some(anchor - Duration::minutes(10));
	town_hall.user_count = Some(12);
	let mut picnic = scheduled_event(
		builder,
		guild_id,
		creator,
		"Community picnic",
		"Meet at the park by the lake.",
		ScheduledEventKind::External,
		ScheduledEventStatus::Scheduled,
	);
	picnic.entity_metadata = Some(ScheduledEventMetadata {
		location: Some("Lakeside Park".to_owned()),
	});
	picnic.scheduled_start_time = Some(anchor + Duration::days(2));
	picnic.scheduled_end_time = Some(anchor + Duration::days(2) + Duration::hours(4));
	picnic.user_count = Some(7);
	vec![town_hall, game_night, picnic]
}

fn voice_candidates(members: &[Member]) -> Vec<&Member> {
	members
		.iter()
		.filter(|member| {
			member
				.user
				.as_ref()
				.is_some_and(|user| user.id.0 != CURRENT_USER_ID)
		})
		.collect()
}

fn stage_state(
	guild_id: u64,
	channel_id: u64,
	member: &Member,
	suppress: bool,
	request_to_speak_timestamp: Option<DateTime<Utc>>,
) -> VoiceState {
	let user_id = member.user.as_ref().map(|user| user.id).unwrap_or_default();
	VoiceState {
		guild_id: Some(Snowflake(guild_id)),
		channel_id: Some(Snowflake(channel_id)),
		user_id,
		member: Some(member.clone()),
		session_id: Some(format!("{:032x}", user_id.0)),
		self_stream: Some(false),
		suppress,
		request_to_speak_timestamp,
		..VoiceState::default()
	}
}

fn build_stage_states(
	guild_id: u64,
	stage: u64,
	members: &[Member],
	taken: &[VoiceState],
	anchor: DateTime<Utc>,
) -> Vec<VoiceState> {
	let free = voice_candidates(members).into_iter().filter(|member| {
		member
			.user
			.as_ref()
			.is_some_and(|user| taken.iter().all(|state| state.user_id != user.id))
	});
	let seats = [
		(false, None),
		(true, Some(anchor - Duration::minutes(2))),
		(true, None),
	];
	free.zip(seats)
		.map(|(member, (suppress, request))| stage_state(guild_id, stage, member, suppress, request))
		.collect()
}

fn build_voice_states(
	builder: &mut Builder,
	guild_id: u64,
	voice: &[u64],
	members: &[Member],
) -> Vec<VoiceState> {
	let candidates = voice_candidates(members);
	if candidates.is_empty() {
		return Vec::new();
	}
	let mut states = Vec::new();
	let mut cursor = 0;
	for &channel_id in voice {
		let limit = builder.channels[&channel_id].user_limit.unwrap_or(0);
		let occupants = if limit > 0 {
			limit as usize
		} else {
			2 + builder.rng.random_range(0..2)
		};
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

	let locked_id = builder.ids.next();
	let mut locked = thread_channel(
		locked_id,
		guild_id,
		first_text,
		"release freeze",
		member_ids[0],
		anchor,
	);
	if let Some(metadata) = locked.thread_metadata.as_mut() {
		metadata.locked = true;
	}
	builder.channels.insert(locked_id, locked);
	thread_ids.push(locked_id);
	let private_id = builder.ids.next();
	let mut private = thread_channel(
		private_id,
		guild_id,
		first_text,
		"mod notes",
		member_ids[0],
		anchor,
	);
	private.kind = ChannelType::PrivateThread;
	private.thread_metadata = private.thread_metadata.map(|metadata| ThreadMetadata {
		archived: true,
		invitable: Some(false),
		..metadata
	});
	builder.channels.insert(private_id, private);
	let private_thread_ids = vec![private_id];

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
		let forum_post = thread.parent_id.map(Snowflake::get) == Some(layout.forum);
		let cap = if forum_post {
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
		let mut thread_messages = messages::generate(&mut builder.rng, &spec);
		let parent = generated
			.iter()
			.find(|(id, _)| Some(*id) == thread.parent_id.map(Snowflake::get))
			.map(|(_, parent_messages)| parent_messages);
		if let Some((id, starter)) = parent
			.filter(|_| !forum_post)
			.and_then(|parent| messages::thread_starter(thread, parent, &thread_messages))
		{
			thread_messages.insert(id, starter);
		}
		generated.push((thread.id.0, thread_messages));
		variant += 1;
	}
	for (channel_id, mut channel_messages) in generated {
		if index == 0 && channel_id == first_text {
			pin_backlog(&mut channel_messages);
		}
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
	let mut voice_states = build_voice_states(builder, guild_id, &layout.voice, &members);
	let stage_states = build_stage_states(guild_id, layout.stage, &members, &voice_states, anchor);
	voice_states.extend(stage_states);
	let stage_instance = StageInstance {
		id: Snowflake(builder.ids.next()),
		guild_id: Snowflake(guild_id),
		channel_id: Snowflake(layout.stage),
		topic: "Community town hall".to_owned(),
		privacy_level: StagePrivacy::GuildOnly,
		guild_scheduled_event_id: None,
	};
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
		features: [
			"COMMUNITY",
			"NEWS",
			"ENHANCED_ROLE_COLORS",
			"ROLE_ICONS",
			"BANNER",
			"INVITE_SPLASH",
		]
		.map(str::to_owned)
		.to_vec(),
		system_channel_flags: Some(SystemChannelFlags::SUPPRESS_PREMIUM_SUBSCRIPTIONS),
		system_channel_id: Some(Snowflake(layout.system)),
		max_members: Some(500_000),
		description: Some("A mock guild for headless runs".to_owned()),
		premium_tier: Some(if index == 0 {
			PremiumTier::Tier2
		} else {
			PremiumTier::Tier1
		}),
		premium_subscription_count: Some(3),
		preferred_locale: Some("en-US".to_owned()),
		nsfw_level: Some(NsfwLevel::Default),
		..GuildProperties::default()
	};
	let scheduled_events = build_scheduled_events(builder, guild_id, &layout, owner.id);
	GuildFixture {
		id: guild_id,
		properties,
		roles,
		channel_ids: layout.all,
		message_channel_ids: layout.message,
		thread_ids,
		private_thread_ids,
		muted_channel_id: layout.muted,
		emojis,
		members,
		member_index,
		presences,
		voice_states,
		scheduled_events,
		stage_instance: Some(stage_instance),
		joined_at: anchor - Duration::days(400),
		large: is_large(index),
		origin: GuildOrigin::Seeded,
	}
}

pub fn build_invitable(builder: &mut Builder) -> GuildFixture {
	let config = builder.config.clone();
	builder.config.messages_per_channel = INVITABLE_MESSAGES;
	builder.config.members_per_guild = INVITABLE_MEMBERS;
	let mut guild = build(builder, 0);
	builder.config = config;
	guild.origin = GuildOrigin::Invitable;
	guild.properties.name = Some(INVITABLE_NAME.to_owned());
	guild.properties.features.push(MEMBER_GATE_FEATURE.to_owned());
	let me = &mut guild.members[0];
	me.roles.clear();
	me.pending = Some(true);
	guild
}

pub fn build_spare(builder: &mut Builder, number: usize) -> GuildFixture {
	let guild_id = builder.ids.next();
	let category_id = builder.ids.next();
	let general_id = builder.ids.next();
	let anchor = builder.anchor;
	let mut everyone = role(guild_id, "@everyone", 0, false, 0, base_permissions());
	everyone.mentionable = false;
	let mut channel_ids = Vec::new();
	let category = guild_channel(
		category_id,
		guild_id,
		ChannelType::GuildCategory,
		"Text Channels",
		0,
		None,
	);
	add_channel(builder, &mut channel_ids, category);
	let general = guild_channel(
		general_id,
		guild_id,
		ChannelType::GuildText,
		"general",
		1,
		Some(category_id),
	);
	add_channel(builder, &mut channel_ids, general);
	builder.messages.insert(general_id, BTreeMap::new());
	let owner = builder.people.current.clone();
	let members = vec![Member {
		user: Some(owner.clone()),
		joined_at: Some(anchor),
		flags: Some(0),
		pending: Some(false),
		guild_id: Some(Snowflake(guild_id)),
		..Member::default()
	}];
	let presences = build_presences(builder, &members);
	let properties = GuildProperties {
		id: Some(Snowflake(guild_id)),
		name: Some(format!("{SPARE_NAME} {number}")),
		owner_id: Some(owner.id),
		verification_level: Some(VerificationLevel::None),
		default_message_notifications: Some(DefaultMessageNotifications::AllMessages),
		explicit_content_filter: Some(ExplicitContentFilter::Disabled),
		system_channel_id: Some(Snowflake(general_id)),
		max_members: Some(500_000),
		premium_tier: Some(PremiumTier::None),
		premium_subscription_count: Some(0),
		preferred_locale: Some("en-US".to_owned()),
		nsfw_level: Some(NsfwLevel::Default),
		..GuildProperties::default()
	};
	GuildFixture {
		id: guild_id,
		properties,
		roles: vec![everyone],
		channel_ids,
		message_channel_ids: vec![general_id],
		thread_ids: Vec::new(),
		private_thread_ids: Vec::new(),
		muted_channel_id: general_id,
		emojis: Vec::new(),
		member_index: HashMap::from([(owner.id.0, 0)]),
		members,
		presences,
		voice_states: Vec::new(),
		scheduled_events: Vec::new(),
		stage_instance: None,
		joined_at: anchor,
		large: false,
		origin: GuildOrigin::Spare,
	}
}
