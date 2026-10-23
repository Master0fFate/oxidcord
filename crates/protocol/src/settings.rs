use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::lenient::{EntryContainer, LenientVec, int_enum, timestamp};
use crate::snowflake::Snowflake;
use crate::user::User;

int_enum! {
	pub enum RelationshipKind {
		Friend = 1,
		Blocked = 2,
		IncomingRequest = 3,
		OutgoingRequest = 4,
		Implicit = 5,
		Suggestion = 6,
	}
}

int_enum! {
	pub enum ReadStateKind {
		Channel = 0,
		GuildEvent = 1,
		NotificationCenter = 2,
		GuildHome = 3,
		Onboarding = 4,
	}
}

int_enum! {
	pub enum NotificationLevel {
		AllMessages = 0,
		OnlyMentions = 1,
		NoMessages = 2,
		Inherit = 3,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadState {
	pub id: Snowflake,
	pub last_message_id: Option<Snowflake>,
	#[serde(deserialize_with = "timestamp")]
	pub last_pin_timestamp: Option<DateTime<Utc>>,
	pub mention_count: Option<i64>,
	pub flags: Option<u64>,
	pub last_viewed: Option<i64>,
	pub read_state_type: Option<ReadStateKind>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MuteConfig {
	#[serde(deserialize_with = "timestamp")]
	pub end_time: Option<DateTime<Utc>>,
	pub selected_time_window: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelOverride {
	pub channel_id: Snowflake,
	pub muted: bool,
	pub mute_config: Option<MuteConfig>,
	pub message_notifications: Option<NotificationLevel>,
	pub collapsed: Option<bool>,
	pub flags: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserGuildSettings {
	pub guild_id: Option<Snowflake>,
	pub muted: bool,
	pub mute_config: Option<MuteConfig>,
	pub message_notifications: Option<NotificationLevel>,
	pub suppress_everyone: bool,
	pub suppress_roles: bool,
	pub hide_muted_channels: bool,
	pub channel_overrides: LenientVec<ChannelOverride>,
	pub notify_highlights: Option<i64>,
	pub flags: Option<u64>,
	pub version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Relationship {
	pub id: Snowflake,
	#[serde(rename = "type")]
	pub kind: RelationshipKind,
	pub user: Option<User>,
	pub user_id: Option<Snowflake>,
	pub nickname: Option<String>,
	#[serde(deserialize_with = "timestamp")]
	pub since: Option<DateTime<Utc>>,
}

pub type ReadStateContainer = EntryContainer<ReadState>;
pub type UserGuildSettingsContainer = EntryContainer<UserGuildSettings>;
