use protocol::Snowflake;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

pub const GATEWAY_VERSION: &str = "8";

pub mod opcode {
	pub const IDENTIFY: u8 = 0;
	pub const SELECT_PROTOCOL: u8 = 1;
	pub const READY: u8 = 2;
	pub const HEARTBEAT: u8 = 3;
	pub const SESSION_DESCRIPTION: u8 = 4;
	pub const SPEAKING: u8 = 5;
	pub const HEARTBEAT_ACK: u8 = 6;
	pub const RESUME: u8 = 7;
	pub const HELLO: u8 = 8;
	pub const RESUMED: u8 = 9;
	pub const CLIENTS_CONNECT: u8 = 11;
	pub const VIDEO: u8 = 12;
	pub const CLIENT_DISCONNECT: u8 = 13;
	pub const MEDIA_SINK_WANTS: u8 = 15;
	pub const DAVE_PREPARE_TRANSITION: u8 = 21;
	pub const DAVE_EXECUTE_TRANSITION: u8 = 22;
	pub const DAVE_TRANSITION_READY: u8 = 23;
	pub const DAVE_PREPARE_EPOCH: u8 = 24;
	pub const DAVE_MLS_EXTERNAL_SENDER: u8 = 25;
	pub const DAVE_MLS_KEY_PACKAGE: u8 = 26;
	pub const DAVE_MLS_PROPOSALS: u8 = 27;
	pub const DAVE_MLS_COMMIT_WELCOME: u8 = 28;
	pub const DAVE_MLS_ANNOUNCE_COMMIT: u8 = 29;
	pub const DAVE_MLS_WELCOME: u8 = 30;
	pub const DAVE_MLS_INVALID_COMMIT_WELCOME: u8 = 31;
}

pub mod close {
	pub const SESSION_INVALID: u16 = 4006;
	pub const SESSION_TIMEOUT: u16 = 4009;
	pub const SERVER_CRASHED: u16 = 4015;
}

#[derive(Debug, Deserialize)]
pub struct RawFrame {
	pub op: u8,
	#[serde(default)]
	pub d: Value,
	pub seq: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct Hello {
	pub heartbeat_interval: f64,
}

#[derive(Debug, Deserialize)]
pub struct Ready {
	pub ssrc: u32,
	pub ip: String,
	pub port: u16,
	#[serde(default)]
	pub modes: Vec<String>,
	#[serde(default, deserialize_with = "lenient_streams")]
	pub streams: Vec<StreamInfo>,
}

fn lenient_streams<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<StreamInfo>, D::Error> {
	let values = Option::<Vec<Value>>::deserialize(deserializer)?;
	Ok(values
		.unwrap_or_default()
		.into_iter()
		.filter_map(|value| serde_json::from_value(value).ok())
		.collect())
}

#[derive(Debug, Deserialize)]
pub struct SessionDescription {
	pub mode: String,
	pub secret_key: Vec<u8>,
	#[serde(default)]
	pub dave_protocol_version: u8,
	pub keyframe_interval: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Speaking {
	#[serde(default)]
	pub speaking: u32,
	pub ssrc: Option<u32>,
	pub user_id: Option<Snowflake>,
}

#[derive(Debug, Deserialize)]
pub struct ClientsConnect {
	#[serde(default)]
	pub user_ids: Vec<Snowflake>,
}

#[derive(Debug, Deserialize)]
pub struct ClientDisconnect {
	pub user_id: Snowflake,
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatAck {
	pub t: u64,
}

#[derive(Debug, Deserialize)]
pub struct PrepareTransition {
	pub transition_id: u16,
	#[serde(default)]
	pub protocol_version: u8,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteTransition {
	pub transition_id: u16,
}

#[derive(Debug, Deserialize)]
pub struct PrepareEpoch {
	#[serde(default)]
	pub protocol_version: u8,
	#[serde(default)]
	pub epoch: u64,
}

#[derive(Debug, Default, Deserialize)]
pub struct StreamInfo {
	#[serde(default)]
	pub ssrc: u32,
	#[serde(default)]
	pub rtx_ssrc: u32,
	pub active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct VideoAnnouncement {
	pub user_id: Option<Snowflake>,
	#[serde(default)]
	pub audio_ssrc: u32,
	#[serde(default)]
	pub video_ssrc: u32,
	#[serde(default, deserialize_with = "lenient_streams")]
	pub streams: Vec<StreamInfo>,
}
