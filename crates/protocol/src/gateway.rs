use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

use crate::channel::{Channel, ChannelType, ThreadMember};
use crate::emoji::{Emoji, PartialEmoji, Sticker};
use crate::error::DecodeError;
use crate::guild::{Guild, UnavailableGuild};
use crate::lenient::{LenientVec, int_enum, timestamp};
use crate::member::Member;
use crate::message::{Message, MessageUpdate};
use crate::presence::Presence;
use crate::role::Role;
use crate::settings::{
	ReadStateContainer, Relationship, RelationshipKind, UserGuildSettings, UserGuildSettingsContainer,
};
use crate::snowflake::Snowflake;
use crate::token::Token;
use crate::user::User;
use crate::voice::VoiceState;

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
		GuildSubscriptions = 14,
		GuildSubscriptionsBulk = 37,
		QosHeartbeat = 40,
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
		"156.0.8078.48"
	};
}

pub const CHROME_VERSION: &str = chrome_version!();
pub const FALLBACK_BUILD_NUMBER: u64 = 647615;

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
	pub browser_user_agent: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub browser_version: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub os_version: Option<String>,
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
	pub design_id: Option<u64>,
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
		Self {
			os: Some(os.to_owned()),
			browser: Some("Chrome".to_owned()),
			device: Some(String::new()),
			system_locale: Some(locale.to_owned()),
			browser_user_agent: Some(chrome_user_agent(chrome_version)),
			browser_version: Some(chrome_version.to_owned()),
			os_version: Some(host_os_version().to_owned()),
			referrer: Some(String::new()),
			referring_domain: Some(String::new()),
			referrer_current: Some(String::new()),
			referring_domain_current: Some(String::new()),
			release_channel: Some("stable".to_owned()),
			client_build_number: Some(build_number),
			client_event_source: Value::Null,
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
	format!("Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{version} Safari/537.36")
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

fn host_os_version() -> &'static str {
	static CACHED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
	CACHED.get_or_init(detect_os_version).as_str()
}

fn detect_os_version() -> String {
	#[cfg(target_os = "windows")]
	{
		use windows::Wdk::System::SystemServices::RtlGetVersion;
		use windows::Win32::Foundation::STATUS_SUCCESS;
		use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
		let mut info = OSVERSIONINFOW {
			dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
			..Default::default()
		};
		if unsafe { RtlGetVersion(&mut info) } == STATUS_SUCCESS {
			format!(
				"{}.{}.{}",
				info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
			)
		} else {
			String::new()
		}
	}
	#[cfg(target_os = "macos")]
	{
		std::process::Command::new("sw_vers")
			.arg("-productVersion")
			.output()
			.ok()
			.and_then(|output| String::from_utf8(output.stdout).ok())
			.map(|value| value.trim().to_owned())
			.filter(|value| !value.is_empty())
			.unwrap_or_default()
	}
	#[cfg(not(any(target_os = "windows", target_os = "macos")))]
	{
		String::new()
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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ready {
	pub v: Option<i64>,
	pub user: User,
	pub guilds: LenientVec<Guild>,
	pub private_channels: LenientVec<Channel>,
	pub relationships: LenientVec<Relationship>,
	pub read_state: ReadStateContainer,
	pub user_guild_settings: UserGuildSettingsContainer,
	pub session_id: String,
	pub resume_gateway_url: Option<String>,
	pub sessions: Vec<Value>,
	pub users: LenientVec<User>,
	pub merged_members: Vec<LenientVec<Member>>,
	pub user_settings_proto: Option<String>,
	pub user_settings: Option<Value>,
}

impl Ready {
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
	#[serde(rename = "type")]
	pub kind: Option<i64>,
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
	MessageReactionRemove(MessageReactionRemove),
	MessageReactionRemoveAll(MessageReactionRemoveAll),
	MessageReactionRemoveEmoji(MessageReactionRemoveEmoji),
	MessageAck(MessageAck),
	ChannelCreate(Box<Channel>),
	ChannelUpdate(Box<Channel>),
	ChannelDelete(Box<Channel>),
	ChannelPinsUpdate(ChannelPinsUpdate),
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
	GuildMembersChunk(Box<GuildMembersChunk>),
	GuildMemberListUpdate(Box<GuildMemberListUpdate>),
	GuildRoleCreate(Box<GuildRoleChange>),
	GuildRoleUpdate(Box<GuildRoleChange>),
	GuildRoleDelete(GuildRoleDelete),
	GuildEmojisUpdate(GuildEmojisUpdate),
	GuildStickersUpdate(GuildStickersUpdate),
	PresenceUpdate(Box<Presence>),
	TypingStart(Box<TypingStart>),
	UserUpdate(Box<User>),
	UserSettingsUpdate(Value),
	UserSettingsProtoUpdate(Value),
	UserGuildSettingsUpdate(Box<UserGuildSettings>),
	RelationshipAdd(Box<Relationship>),
	RelationshipRemove(RelationshipRemove),
	VoiceStateUpdate(Box<VoiceState>),
	VoiceServerUpdate(VoiceServerUpdate),
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
			"MESSAGE_REACTION_REMOVE" => Self::MessageReactionRemove(decode(name, data)?),
			"MESSAGE_REACTION_REMOVE_ALL" => Self::MessageReactionRemoveAll(decode(name, data)?),
			"MESSAGE_REACTION_REMOVE_EMOJI" => Self::MessageReactionRemoveEmoji(decode(name, data)?),
			"MESSAGE_ACK" => Self::MessageAck(decode(name, data)?),
			"CHANNEL_CREATE" => Self::ChannelCreate(decode(name, data)?),
			"CHANNEL_UPDATE" => Self::ChannelUpdate(decode(name, data)?),
			"CHANNEL_DELETE" => Self::ChannelDelete(decode(name, data)?),
			"CHANNEL_PINS_UPDATE" => Self::ChannelPinsUpdate(decode(name, data)?),
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
			"GUILD_MEMBERS_CHUNK" => Self::GuildMembersChunk(decode(name, data)?),
			"GUILD_MEMBER_LIST_UPDATE" => Self::GuildMemberListUpdate(decode(name, data)?),
			"GUILD_ROLE_CREATE" => Self::GuildRoleCreate(decode(name, data)?),
			"GUILD_ROLE_UPDATE" => Self::GuildRoleUpdate(decode(name, data)?),
			"GUILD_ROLE_DELETE" => Self::GuildRoleDelete(decode(name, data)?),
			"GUILD_EMOJIS_UPDATE" => Self::GuildEmojisUpdate(decode(name, data)?),
			"GUILD_STICKERS_UPDATE" => Self::GuildStickersUpdate(decode(name, data)?),
			"PRESENCE_UPDATE" => Self::PresenceUpdate(decode(name, data)?),
			"TYPING_START" => Self::TypingStart(decode(name, data)?),
			"USER_UPDATE" => Self::UserUpdate(decode(name, data)?),
			"USER_SETTINGS_UPDATE" => Self::UserSettingsUpdate(decode(name, data)?),
			"USER_SETTINGS_PROTO_UPDATE" => Self::UserSettingsProtoUpdate(decode(name, data)?),
			"USER_GUILD_SETTINGS_UPDATE" => Self::UserGuildSettingsUpdate(decode(name, data)?),
			"RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => Self::RelationshipAdd(decode(name, data)?),
			"RELATIONSHIP_REMOVE" => Self::RelationshipRemove(decode(name, data)?),
			"VOICE_STATE_UPDATE" => Self::VoiceStateUpdate(decode(name, data)?),
			"VOICE_SERVER_UPDATE" => Self::VoiceServerUpdate(decode(name, data)?),
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
