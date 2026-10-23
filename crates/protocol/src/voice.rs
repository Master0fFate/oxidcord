use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::lenient::timestamp;
use crate::member::Member;
use crate::snowflake::Snowflake;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceState {
	pub guild_id: Option<Snowflake>,
	pub channel_id: Option<Snowflake>,
	pub user_id: Snowflake,
	pub member: Option<Member>,
	pub session_id: Option<String>,
	pub deaf: bool,
	pub mute: bool,
	pub self_deaf: bool,
	pub self_mute: bool,
	pub self_stream: Option<bool>,
	pub self_video: bool,
	pub suppress: bool,
	#[serde(deserialize_with = "timestamp")]
	pub request_to_speak_timestamp: Option<DateTime<Utc>>,
}
