use std::mem;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

use crate::channel::{Channel, ChannelType, ThreadMember};
use crate::command::CommandAutocomplete;
use crate::emoji::{Emoji, PartialEmoji, Sticker};
use crate::error::DecodeError;
use crate::guild::{Guild, ScheduledEvent, UnavailableGuild};
use crate::lenient::{LenientVec, int_enum, or_default, timestamp, timestamp_or_seconds};
use crate::member::Member;
use crate::message::{BURST_REACTION, Message, MessageUpdate};
use crate::presence::Presence;
use crate::role::Role;
use crate::settings::{
	ReadStateContainer, Relationship, RelationshipKind, RelationshipUpdate, UserGuildSettings,
	UserGuildSettingsContainer, UserNoteUpdate,
};
use crate::snowflake::Snowflake;
use crate::soundboard::{SoundboardSound, SoundboardSoundDelete, SoundboardSounds, VoiceChannelEffect};
use crate::token::Token;
use crate::user::User;
use crate::verification::{JoinRequestChange, JoinRequestDelete};
use crate::voice::{StageInstance, VoiceState};

int_enum! {
	pub enum Opcode {
		Dispatch = 0,
		Heartbeat = 1,
		Identify = 2,
		PresenceUpdate = 3,
		VoiceStateUpdate = 4,
		Resume = 6,
		Reconnect = 7,
		RequestGuildMembers = 8,
		InvalidSession = 9,
		Hello = 10,
		HeartbeatAck = 11,
		StreamCreate = 18,
		StreamDelete = 19,
		StreamWatch = 20,
		StreamPing = 21,
		StreamSetPaused = 22,
		RequestSoundboardSounds = 31,
		GuildSubscriptions = 14,
		GuildSubscriptionsBulk = 37,
		QosHeartbeat = 40,
		RequestChannelInfo = 43,
	}
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayPayload {
	pub op: i64,
	#[serde(default)]
	pub d: Option<Box<RawValue>>,
	#[serde(default)]
	pub s: Option<u64>,
	#[serde(default)]
	pub t: Option<String>,
}

impl GatewayPayload {
	pub fn new<T: Serialize>(opcode: Opcode, data: &T) -> Result<Self, serde_json::Error> {
		Ok(Self {
			op: opcode.into(),
			d: Some(serde_json::value::to_raw_value(data)?),
			s: None,
			t: None,
		})
	}

	pub fn opcode(&self) -> Opcode {
		Opcode::from(self.op)
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
	pub heartbeat_interval: u64,
}

macro_rules! chrome_version {
	() => {
		"156.0.0.0"
	};
}

pub const CHROME_VERSION: &str = chrome_version!();
pub const FALLBACK_BUILD_NUMBER: u64 = 655747;

const WINDOWS_USER_AGENT: &str = concat!(
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
	chrome_version!(),
	" Safari/537.36"
);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IdentifyProperties {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub os: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub browser: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub device: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub system_locale: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub has_client_mods: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub browser_user_agent: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub browser_version: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub os_version: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub os_arch: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub referrer: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub referring_domain: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub referrer_current: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub referring_domain_current: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub release_channel: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub client_build_number: Option<u64>,
	pub client_event_source: Value,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub client_launch_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub launch_signature: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub client_heartbeat_session_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub client_app_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub design_id: Option<u64>,
}

struct LaunchIds {
	launch: String,
	signature: String,
	heartbeat_session: String,
}

fn launch_ids() -> &'static LaunchIds {
	static CACHED: std::sync::OnceLock<LaunchIds> = std::sync::OnceLock::new();
	CACHED.get_or_init(|| LaunchIds {
		launch: random_uuid(),
		signature: random_uuid(),
		heartbeat_session: random_uuid(),
	})
}

fn random_uuid() -> String {
	let bits = (rand::random::<u128>() & !(0xf000u128 << 64) & !(0xc000u128 << 48))
		| (0x4000u128 << 64)
		| (0x8000u128 << 48);
	let hex = format!("{bits:032x}");
	format!(
		"{}-{}-{}-{}-{}",
		&hex[..8],
		&hex[8..12],
		&hex[12..16],
		&hex[16..20],
		&hex[20..]
	)
}

pub fn reduced_chrome_version(version: &str) -> String {
	let major = version.split('.').next().unwrap_or_default();
	format!("{major}.0.0.0")
}

impl IdentifyProperties {
	pub fn web_client(locale: &str, build_number: u64, chrome_version: &str) -> Self {
		let os = if cfg!(target_os = "macos") {
			"Mac OS X"
		} else if cfg!(target_os = "linux") {
			"Linux"
		} else {
			"Windows"
		};
		let ids = launch_ids();
		Self {
			os: Some(os.to_owned()),
			browser: Some("Chrome".to_owned()),
			device: Some(String::new()),
			system_locale: Some(locale.to_owned()),
			has_client_mods: Some(false),
			browser_user_agent: Some(chrome_user_agent(chrome_version)),
			browser_version: Some(reduced_chrome_version(chrome_version)),
			os_version: Some(browser_os_version().to_owned()),
			os_arch: None,
			referrer: Some(String::new()),
			referring_domain: Some(String::new()),
			referrer_current: Some(String::new()),
			referring_domain_current: Some(String::new()),
			release_channel: Some("stable".to_owned()),
			client_build_number: Some(build_number),
			client_event_source: Value::Null,
			client_launch_id: Some(ids.launch.clone()),
			launch_signature: Some(ids.signature.clone()),
			client_heartbeat_session_id: Some(ids.heartbeat_session.clone()),
			client_app_state: Some("focused".to_owned()),
			design_id: None,
		}
	}

	pub fn super_properties(&self) -> String {
		STANDARD.encode(serde_json::to_vec(self).unwrap_or_default())
	}

	pub fn user_agent(&self) -> &str {
		self.browser_user_agent.as_deref().unwrap_or(WINDOWS_USER_AGENT)
	}
}

pub fn chrome_user_agent(version: &str) -> String {
	let platform = if cfg!(target_os = "macos") {
		"Macintosh; Intel Mac OS X 10_15_7"
	} else if cfg!(target_os = "linux") {
		"X11; Linux x86_64"
	} else {
		"Windows NT 10.0; Win64; x64"
	};
	let version = reduced_chrome_version(version);
	format!("Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{version} Safari/537.36")
}

fn browser_os_version() -> &'static str {
	if cfg!(target_os = "macos") {
		"10.15.7"
	} else if cfg!(target_os = "linux") {
		""
	} else {
		"10"
	}
}

pub fn web_user_agent() -> String {
	chrome_user_agent(CHROME_VERSION)
}

pub fn accept_language(locale: &str) -> String {
	format!("{locale},en;q=0.9")
}

pub fn sec_ch_ua(version: &str) -> String {
	let major = version.split('.').next().unwrap_or_default();
	format!("\"Chromium\";v=\"{major}\", \"Not/A)Brand\";v=\"99\", \"Google Chrome\";v=\"{major}\"")
}

pub fn sec_ch_ua_platform() -> &'static str {
	if cfg!(target_os = "macos") {
		"macOS"
	} else if cfg!(target_os = "linux") {
		"Linux"
	} else {
		"Windows"
	}
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Identify {
	pub token: Token,
	pub properties: IdentifyProperties,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub compress: Option<bool>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub large_threshold: Option<u32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub presence: Option<Value>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub capabilities: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub client_state: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Resume {
	pub token: Token,
	pub session_id: String,
	pub seq: u64,
}

pub mod capability {
	pub const LAZY_USER_NOTES: u64 = 1 << 0;
	pub const VERSIONED_READ_STATES: u64 = 1 << 2;
	pub const VERSIONED_USER_GUILD_SETTINGS: u64 = 1 << 3;
	pub const DEDUPE_USER_OBJECTS: u64 = 1 << 4;
	pub const PRIORITIZED_READY_PAYLOAD: u64 = 1 << 5;
	pub const MULTIPLE_GUILD_EXPERIMENT_POPULATIONS: u64 = 1 << 6;
	pub const NON_CHANNEL_READ_STATES: u64 = 1 << 7;
	pub const AUTH_TOKEN_REFRESH: u64 = 1 << 8;
	pub const USER_SETTINGS_PROTO: u64 = 1 << 9;
	pub const CLIENT_STATE_V2: u64 = 1 << 10;
	pub const PASSIVE_GUILD_UPDATE: u64 = 1 << 11;
	pub const AUTO_CALL_CONNECT: u64 = 1 << 12;
	pub const DEBOUNCE_MESSAGE_REACTIONS: u64 = 1 << 13;
	pub const PASSIVE_GUILD_UPDATE_V2: u64 = 1 << 14;
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ready {
	#[serde(deserialize_with = "or_default")]
	pub v: Option<i64>,
	pub user: User,
	pub guilds: LenientVec<Guild>,
	pub private_channels: LenientVec<Channel>,
	pub relationships: LenientVec<Relationship>,
	pub read_state: ReadStateContainer,
	pub user_guild_settings: UserGuildSettingsContainer,
	#[serde(deserialize_with = "or_default")]
	pub session_id: String,
	#[serde(deserialize_with = "or_default")]
	pub auth_session_id_hash: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub resume_gateway_url: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub sessions: Vec<Value>,
	pub users: LenientVec<User>,
	#[serde(deserialize_with = "or_default")]
	pub merged_members: Vec<LenientVec<Member>>,
	#[serde(deserialize_with = "or_default")]
	pub user_settings_proto: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub user_settings: Option<Value>,
	#[serde(skip_serializing, deserialize_with = "or_default")]
	pub auth_token: Option<Token>,
}

impl Ready {
	pub fn align_merged_members(&mut self) {
		if self.guilds.dropped() == 0 {
			return;
		}
		let mut merged = mem::take(&mut self.merged_members);
		self.merged_members = self
			.guilds
			.iter_with_source()
			.map(|(source, _)| merged.get_mut(source).map(mem::take).unwrap_or_default())
			.collect();
	}

	pub fn normalize_guilds(&mut self) {
		for guild in self.guilds.iter_mut() {
			*guild = std::mem::take(guild).normalized();
		}
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MergedPresences {
	pub guilds: Vec<LenientVec<Presence>>,
	pub friends: LenientVec<Presence>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SupplementalGuild {
	pub id: Snowflake,
	pub voice_states: LenientVec<VoiceState>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadySupplemental {
	pub merged_presences: MergedPresences,
	pub merged_members: Vec<LenientVec<Member>>,
	pub guilds: LenientVec<SupplementalGuild>,
	pub lazy_private_channels: LenientVec<Channel>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageDelete {
	pub id: Snowflake,
	pub channel_id: Snowflake,
	pub guild_id: Option<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageDeleteBulk {
	pub ids: Vec<Snowflake>,
	pub channel_id: Snowflake,
	pub guild_id: Option<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageReactionAdd {
	pub user_id: Snowflake,
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub member: Option<Member>,
	pub emoji: PartialEmoji,
	pub message_author_id: Option<Snowflake>,
	pub burst: Option<bool>,
	#[serde(deserialize_with = "or_default")]
	pub burst_colors: Vec<String>,
	#[serde(rename = "type")]
	pub kind: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DebouncedReaction {
	pub emoji: PartialEmoji,
	pub users: Vec<Snowflake>,
	pub burst: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageReactionAddMany {
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub reactions: LenientVec<DebouncedReaction>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageReactionRemove {
	pub user_id: Snowflake,
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub emoji: PartialEmoji,
	pub burst: Option<bool>,
	#[serde(rename = "type")]
	pub kind: Option<i64>,
}

impl MessageReactionAdd {
	pub fn is_burst(&self) -> bool {
		self.burst == Some(true) || self.kind == Some(BURST_REACTION)
	}
}

impl MessageReactionRemove {
	pub fn is_burst(&self) -> bool {
		self.burst == Some(true) || self.kind == Some(BURST_REACTION)
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessagePollVote {
	pub user_id: Snowflake,
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub answer_id: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageReactionRemoveAll {
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub guild_id: Option<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageReactionRemoveEmoji {
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub emoji: PartialEmoji,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelRecipientChange {
	pub channel_id: Snowflake,
	pub user: User,
	pub nick: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageAck {
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub version: Option<i64>,
	pub mention_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelPinsUpdate {
	pub guild_id: Option<Snowflake>,
	pub channel_id: Snowflake,
	#[serde(deserialize_with = "timestamp")]
	pub last_pin_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelPinsAck {
	pub channel_id: Snowflake,
	#[serde(deserialize_with = "timestamp")]
	pub timestamp: Option<DateTime<Utc>>,
	pub version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelLastMessage {
	pub id: Snowflake,
	pub last_message_id: Option<Snowflake>,
	#[serde(deserialize_with = "timestamp")]
	pub last_pin_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PassiveUpdate {
	pub guild_id: Snowflake,
	pub updated_channels: LenientVec<ChannelLastMessage>,
	pub updated_voice_states: LenientVec<VoiceState>,
	pub removed_voice_states: LenientVec<Snowflake>,
	pub updated_members: LenientVec<Member>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelUnreadUpdate {
	pub guild_id: Snowflake,
	pub channel_unread_updates: LenientVec<ChannelLastMessage>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceChannelStatusUpdate {
	pub id: Snowflake,
	pub guild_id: Snowflake,
	pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceChannelStartTimeUpdate {
	pub id: Snowflake,
	pub guild_id: Snowflake,
	#[serde(deserialize_with = "timestamp_or_seconds")]
	pub voice_start_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelInfoEntry {
	pub id: Snowflake,
	pub status: Option<String>,
	#[serde(deserialize_with = "timestamp_or_seconds")]
	pub voice_start_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelInfo {
	pub guild_id: Snowflake,
	pub channels: LenientVec<ChannelInfoEntry>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadDelete {
	pub id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub parent_id: Option<Snowflake>,
	#[serde(rename = "type")]
	pub kind: ChannelType,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadListSync {
	pub guild_id: Snowflake,
	pub channel_ids: Option<Vec<Snowflake>>,
	pub threads: LenientVec<Channel>,
	pub members: LenientVec<ThreadMember>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadMembersUpdate {
	pub id: Snowflake,
	pub guild_id: Snowflake,
	pub member_count: Option<i64>,
	pub added_members: LenientVec<ThreadMember>,
	pub removed_member_ids: Vec<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildMemberRemove {
	pub guild_id: Snowflake,
	pub user: User,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildBan {
	pub guild_id: Snowflake,
	pub user: User,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildMembersChunk {
	pub guild_id: Snowflake,
	pub members: LenientVec<Member>,
	pub chunk_index: Option<i64>,
	pub chunk_count: Option<i64>,
	pub not_found: Vec<Snowflake>,
	pub presences: LenientVec<Presence>,
	pub nonce: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemberListGroup {
	pub id: String,
	pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildMemberListUpdate {
	pub guild_id: Snowflake,
	pub id: String,
	pub ops: Vec<Value>,
	pub groups: LenientVec<MemberListGroup>,
	pub member_count: Option<i64>,
	pub online_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildRoleChange {
	pub guild_id: Snowflake,
	pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildRoleDelete {
	pub guild_id: Snowflake,
	pub role_id: Snowflake,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildEmojisUpdate {
	pub guild_id: Snowflake,
	pub emojis: LenientVec<Emoji>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildStickersUpdate {
	pub guild_id: Snowflake,
	pub stickers: LenientVec<Sticker>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledEventUser {
	pub guild_scheduled_event_id: Snowflake,
	pub guild_id: Snowflake,
	pub user_id: Snowflake,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TypingStart {
	pub channel_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub user_id: Snowflake,
	pub timestamp: Option<i64>,
	pub member: Option<Member>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RelationshipRemove {
	pub id: Snowflake,
	#[serde(rename = "type")]
	pub kind: Option<RelationshipKind>,
	pub nickname: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceServerUpdate {
	pub token: Option<Token>,
	pub guild_id: Option<Snowflake>,
	pub channel_id: Option<Snowflake>,
	pub endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamState {
	pub stream_key: String,
	pub rtc_server_id: Option<Snowflake>,
	pub rtc_channel_id: Option<Snowflake>,
	pub region: Option<String>,
	pub viewer_ids: Vec<Snowflake>,
	pub paused: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamServerUpdate {
	pub stream_key: String,
	pub token: Option<Token>,
	pub guild_id: Option<Snowflake>,
	pub endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamDelete {
	pub stream_key: String,
	pub reason: Option<String>,
	pub unavailable: Option<bool>,
}

#[derive(Debug)]
pub enum DispatchEvent {
	Ready(Box<Ready>),
	ReadySupplemental(Box<ReadySupplemental>),
	Resumed,
	MessageCreate(Box<Message>),
	MessageUpdate(Box<MessageUpdate>),
	MessageDelete(MessageDelete),
	MessageDeleteBulk(MessageDeleteBulk),
	MessageReactionAdd(Box<MessageReactionAdd>),
	MessageReactionAddMany(Box<MessageReactionAddMany>),
	MessageReactionRemove(MessageReactionRemove),
	MessageReactionRemoveAll(MessageReactionRemoveAll),
	MessageReactionRemoveEmoji(MessageReactionRemoveEmoji),
	MessagePollVoteAdd(MessagePollVote),
	MessagePollVoteRemove(MessagePollVote),
	MessageAck(MessageAck),
	ChannelCreate(Box<Channel>),
	ChannelUpdate(Box<Channel>),
	ChannelDelete(Box<Channel>),
	ChannelPinsUpdate(ChannelPinsUpdate),
	ChannelPinsAck(ChannelPinsAck),
	ChannelUnreadUpdate(ChannelUnreadUpdate),
	VoiceChannelStatusUpdate(VoiceChannelStatusUpdate),
	VoiceChannelStartTimeUpdate(VoiceChannelStartTimeUpdate),
	ChannelInfo(ChannelInfo),
	PassiveUpdateV2(Box<PassiveUpdate>),
	ChannelRecipientAdd(Box<ChannelRecipientChange>),
	ChannelRecipientRemove(Box<ChannelRecipientChange>),
	ThreadCreate(Box<Channel>),
	ThreadUpdate(Box<Channel>),
	ThreadDelete(ThreadDelete),
	ThreadListSync(Box<ThreadListSync>),
	ThreadMemberUpdate(Box<ThreadMember>),
	ThreadMembersUpdate(Box<ThreadMembersUpdate>),
	GuildCreate(Box<Guild>),
	GuildUpdate(Box<Guild>),
	GuildDelete(UnavailableGuild),
	GuildMemberAdd(Box<Member>),
	GuildMemberUpdate(Box<Member>),
	GuildMemberRemove(Box<GuildMemberRemove>),
	GuildBanAdd(Box<GuildBan>),
	GuildBanRemove(Box<GuildBan>),
	GuildMembersChunk(Box<GuildMembersChunk>),
	GuildMemberListUpdate(Box<GuildMemberListUpdate>),
	GuildRoleCreate(Box<GuildRoleChange>),
	GuildRoleUpdate(Box<GuildRoleChange>),
	GuildRoleDelete(GuildRoleDelete),
	GuildEmojisUpdate(GuildEmojisUpdate),
	GuildStickersUpdate(GuildStickersUpdate),
	SoundboardSounds(SoundboardSounds),
	GuildSoundboardSoundCreate(Box<SoundboardSound>),
	GuildSoundboardSoundUpdate(Box<SoundboardSound>),
	GuildSoundboardSoundDelete(SoundboardSoundDelete),
	VoiceChannelEffectSend(Box<VoiceChannelEffect>),
	ScheduledEventCreate(Box<ScheduledEvent>),
	ScheduledEventUpdate(Box<ScheduledEvent>),
	ScheduledEventDelete(Box<ScheduledEvent>),
	ScheduledEventUserAdd(ScheduledEventUser),
	ScheduledEventUserRemove(ScheduledEventUser),
	GuildJoinRequestCreate(Box<JoinRequestChange>),
	GuildJoinRequestUpdate(Box<JoinRequestChange>),
	GuildJoinRequestDelete(JoinRequestDelete),
	PresenceUpdate(Box<Presence>),
	TypingStart(Box<TypingStart>),
	UserUpdate(Box<User>),
	UserSettingsUpdate(Value),
	UserSettingsProtoUpdate(Value),
	UserConnectionsUpdate(Value),
	UserGuildSettingsUpdate(Box<UserGuildSettings>),
	RelationshipAdd(Box<Relationship>),
	RelationshipUpdate(Box<RelationshipUpdate>),
	UserNoteUpdate(UserNoteUpdate),
	RelationshipRemove(RelationshipRemove),
	VoiceStateUpdate(Box<VoiceState>),
	VoiceServerUpdate(VoiceServerUpdate),
	StageInstanceCreate(Box<StageInstance>),
	StageInstanceUpdate(Box<StageInstance>),
	StageInstanceDelete(Box<StageInstance>),
	StreamCreate(Box<StreamState>),
	StreamUpdate(Box<StreamState>),
	StreamServerUpdate(StreamServerUpdate),
	StreamDelete(StreamDelete),
	CallCreate(Value),
	CallUpdate(Value),
	CallDelete(Value),
	SessionsReplace(Vec<Value>),
	InteractionCreate(Value),
	InteractionSuccess(Value),
	InteractionFailure(Value),
	ApplicationCommandAutocompleteResponse(CommandAutocomplete),
	Unknown { name: String, data: Box<RawValue> },
}

fn decode<T: DeserializeOwned>(name: &str, data: &RawValue) -> Result<T, DecodeError> {
	serde_json::from_str(data.get()).map_err(|error| DecodeError::new(name, error))
}

impl DispatchEvent {
	pub fn parse(name: &str, data: &RawValue) -> Result<Self, DecodeError> {
		let event = match name {
			"READY" => {
				let mut ready: Box<Ready> = decode(name, data)?;
				ready.normalize_guilds();
				Self::Ready(ready)
			}
			"READY_SUPPLEMENTAL" => Self::ReadySupplemental(decode(name, data)?),
			"RESUMED" => Self::Resumed,
			"MESSAGE_CREATE" => Self::MessageCreate(decode(name, data)?),
			"MESSAGE_UPDATE" => Self::MessageUpdate(decode(name, data)?),
			"MESSAGE_DELETE" => Self::MessageDelete(decode(name, data)?),
			"MESSAGE_DELETE_BULK" => Self::MessageDeleteBulk(decode(name, data)?),
			"MESSAGE_REACTION_ADD" => Self::MessageReactionAdd(decode(name, data)?),
			"MESSAGE_REACTION_ADD_MANY" => Self::MessageReactionAddMany(decode(name, data)?),
			"MESSAGE_REACTION_REMOVE" => Self::MessageReactionRemove(decode(name, data)?),
			"MESSAGE_REACTION_REMOVE_ALL" => Self::MessageReactionRemoveAll(decode(name, data)?),
			"MESSAGE_REACTION_REMOVE_EMOJI" => Self::MessageReactionRemoveEmoji(decode(name, data)?),
			"MESSAGE_POLL_VOTE_ADD" => Self::MessagePollVoteAdd(decode(name, data)?),
			"MESSAGE_POLL_VOTE_REMOVE" => Self::MessagePollVoteRemove(decode(name, data)?),
			"MESSAGE_ACK" => Self::MessageAck(decode(name, data)?),
			"CHANNEL_CREATE" => Self::ChannelCreate(decode(name, data)?),
			"CHANNEL_UPDATE" => Self::ChannelUpdate(decode(name, data)?),
			"CHANNEL_DELETE" => Self::ChannelDelete(decode(name, data)?),
			"CHANNEL_PINS_UPDATE" => Self::ChannelPinsUpdate(decode(name, data)?),
			"CHANNEL_PINS_ACK" => Self::ChannelPinsAck(decode(name, data)?),
			"CHANNEL_UNREAD_UPDATE" => Self::ChannelUnreadUpdate(decode(name, data)?),
			"VOICE_CHANNEL_STATUS_UPDATE" => Self::VoiceChannelStatusUpdate(decode(name, data)?),
			"VOICE_CHANNEL_START_TIME_UPDATE" => Self::VoiceChannelStartTimeUpdate(decode(name, data)?),
			"CHANNEL_INFO" => Self::ChannelInfo(decode(name, data)?),
			"PASSIVE_UPDATE_V2" => Self::PassiveUpdateV2(decode(name, data)?),
			"CHANNEL_RECIPIENT_ADD" => Self::ChannelRecipientAdd(decode(name, data)?),
			"CHANNEL_RECIPIENT_REMOVE" => Self::ChannelRecipientRemove(decode(name, data)?),
			"THREAD_CREATE" => Self::ThreadCreate(decode(name, data)?),
			"THREAD_UPDATE" => Self::ThreadUpdate(decode(name, data)?),
			"THREAD_DELETE" => Self::ThreadDelete(decode(name, data)?),
			"THREAD_LIST_SYNC" => Self::ThreadListSync(decode(name, data)?),
			"THREAD_MEMBER_UPDATE" => Self::ThreadMemberUpdate(decode(name, data)?),
			"THREAD_MEMBERS_UPDATE" => Self::ThreadMembersUpdate(decode(name, data)?),
			"GUILD_CREATE" => Self::GuildCreate(decode_guild(name, data)?),
			"GUILD_UPDATE" => Self::GuildUpdate(decode_guild(name, data)?),
			"GUILD_DELETE" => Self::GuildDelete(decode(name, data)?),
			"GUILD_MEMBER_ADD" => Self::GuildMemberAdd(decode(name, data)?),
			"GUILD_MEMBER_UPDATE" => Self::GuildMemberUpdate(decode(name, data)?),
			"GUILD_MEMBER_REMOVE" => Self::GuildMemberRemove(decode(name, data)?),
			"GUILD_BAN_ADD" => Self::GuildBanAdd(decode(name, data)?),
			"GUILD_BAN_REMOVE" => Self::GuildBanRemove(decode(name, data)?),
			"GUILD_MEMBERS_CHUNK" => Self::GuildMembersChunk(decode(name, data)?),
			"GUILD_MEMBER_LIST_UPDATE" => Self::GuildMemberListUpdate(decode(name, data)?),
			"GUILD_ROLE_CREATE" => Self::GuildRoleCreate(decode(name, data)?),
			"GUILD_ROLE_UPDATE" => Self::GuildRoleUpdate(decode(name, data)?),
			"GUILD_ROLE_DELETE" => Self::GuildRoleDelete(decode(name, data)?),
			"GUILD_EMOJIS_UPDATE" => Self::GuildEmojisUpdate(decode(name, data)?),
			"GUILD_STICKERS_UPDATE" => Self::GuildStickersUpdate(decode(name, data)?),
			"SOUNDBOARD_SOUNDS" | "GUILD_SOUNDBOARD_SOUNDS_UPDATE" => {
				Self::SoundboardSounds(decode(name, data)?)
			}
			"GUILD_SOUNDBOARD_SOUND_CREATE" => Self::GuildSoundboardSoundCreate(decode(name, data)?),
			"GUILD_SOUNDBOARD_SOUND_UPDATE" => Self::GuildSoundboardSoundUpdate(decode(name, data)?),
			"GUILD_SOUNDBOARD_SOUND_DELETE" => Self::GuildSoundboardSoundDelete(decode(name, data)?),
			"VOICE_CHANNEL_EFFECT_SEND" => Self::VoiceChannelEffectSend(decode(name, data)?),
			"GUILD_SCHEDULED_EVENT_CREATE" => Self::ScheduledEventCreate(decode(name, data)?),
			"GUILD_SCHEDULED_EVENT_UPDATE" => Self::ScheduledEventUpdate(decode(name, data)?),
			"GUILD_SCHEDULED_EVENT_DELETE" => Self::ScheduledEventDelete(decode(name, data)?),
			"GUILD_SCHEDULED_EVENT_USER_ADD" => Self::ScheduledEventUserAdd(decode(name, data)?),
			"GUILD_SCHEDULED_EVENT_USER_REMOVE" => Self::ScheduledEventUserRemove(decode(name, data)?),
			"GUILD_JOIN_REQUEST_CREATE" => Self::GuildJoinRequestCreate(decode(name, data)?),
			"GUILD_JOIN_REQUEST_UPDATE" => Self::GuildJoinRequestUpdate(decode(name, data)?),
			"GUILD_JOIN_REQUEST_DELETE" => Self::GuildJoinRequestDelete(decode(name, data)?),
			"PRESENCE_UPDATE" => Self::PresenceUpdate(decode(name, data)?),
			"TYPING_START" => Self::TypingStart(decode(name, data)?),
			"USER_UPDATE" => Self::UserUpdate(decode(name, data)?),
			"USER_SETTINGS_UPDATE" => Self::UserSettingsUpdate(decode(name, data)?),
			"USER_SETTINGS_PROTO_UPDATE" => Self::UserSettingsProtoUpdate(decode(name, data)?),
			"USER_CONNECTIONS_UPDATE" => Self::UserConnectionsUpdate(decode(name, data)?),
			"USER_GUILD_SETTINGS_UPDATE" => Self::UserGuildSettingsUpdate(decode(name, data)?),
			"RELATIONSHIP_ADD" => Self::RelationshipAdd(decode(name, data)?),
			"RELATIONSHIP_UPDATE" => Self::RelationshipUpdate(decode(name, data)?),
			"USER_NOTE_UPDATE" => Self::UserNoteUpdate(decode(name, data)?),
			"RELATIONSHIP_REMOVE" => Self::RelationshipRemove(decode(name, data)?),
			"VOICE_STATE_UPDATE" => Self::VoiceStateUpdate(decode(name, data)?),
			"VOICE_SERVER_UPDATE" => Self::VoiceServerUpdate(decode(name, data)?),
			"STAGE_INSTANCE_CREATE" => Self::StageInstanceCreate(decode(name, data)?),
			"STAGE_INSTANCE_UPDATE" => Self::StageInstanceUpdate(decode(name, data)?),
			"STAGE_INSTANCE_DELETE" => Self::StageInstanceDelete(decode(name, data)?),
			"STREAM_CREATE" => Self::StreamCreate(decode(name, data)?),
			"STREAM_UPDATE" => Self::StreamUpdate(decode(name, data)?),
			"STREAM_SERVER_UPDATE" => Self::StreamServerUpdate(decode(name, data)?),
			"STREAM_DELETE" => Self::StreamDelete(decode(name, data)?),
			"CALL_CREATE" => Self::CallCreate(decode(name, data)?),
			"CALL_UPDATE" => Self::CallUpdate(decode(name, data)?),
			"CALL_DELETE" => Self::CallDelete(decode(name, data)?),
			"SESSIONS_REPLACE" => Self::SessionsReplace(decode(name, data)?),
			"INTERACTION_CREATE" => Self::InteractionCreate(decode(name, data)?),
			"INTERACTION_SUCCESS" => Self::InteractionSuccess(decode(name, data)?),
			"INTERACTION_FAILURE" => Self::InteractionFailure(decode(name, data)?),
			"APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE" => {
				Self::ApplicationCommandAutocompleteResponse(decode(name, data)?)
			}
			_ => Self::Unknown {
				name: name.to_owned(),
				data: data.to_owned(),
			},
		};
		Ok(event)
	}
}

fn decode_guild(name: &str, data: &RawValue) -> Result<Box<Guild>, DecodeError> {
	let guild: Guild = decode(name, data)?;
	Ok(Box::new(guild.normalized()))
}
