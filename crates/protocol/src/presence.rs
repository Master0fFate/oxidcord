use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::emoji::PartialEmoji;
use crate::lenient::{LenientVec, int_enum, str_enum};
use crate::snowflake::Snowflake;
use crate::user::User;

str_enum! {
	pub enum StatusKind {
		Offline = "offline",
		Online = "online",
		Idle = "idle",
		Dnd = "dnd",
		Invisible = "invisible",
	}
}

int_enum! {
	pub enum ActivityKind {
		Playing = 0,
		Streaming = 1,
		Listening = 2,
		Watching = 3,
		Custom = 4,
		Competing = 5,
		HangStatus = 6,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientStatus {
	pub desktop: Option<StatusKind>,
	pub mobile: Option<StatusKind>,
	pub web: Option<StatusKind>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityTimestamps {
	pub start: Option<i64>,
	pub end: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityParty {
	pub id: Option<String>,
	pub size: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityAssets {
	pub large_image: Option<String>,
	pub large_text: Option<String>,
	pub small_image: Option<String>,
	pub small_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivitySecrets {
	pub join: Option<String>,
	pub spectate: Option<String>,
	#[serde(rename = "match")]
	pub match_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
	pub id: Option<String>,
	pub name: String,
	#[serde(rename = "type")]
	pub kind: ActivityKind,
	pub url: Option<String>,
	pub created_at: Option<i64>,
	pub timestamps: Option<ActivityTimestamps>,
	pub application_id: Option<Snowflake>,
	pub details: Option<String>,
	pub state: Option<String>,
	pub emoji: Option<PartialEmoji>,
	pub party: Option<ActivityParty>,
	pub assets: Option<ActivityAssets>,
	pub secrets: Option<ActivitySecrets>,
	pub instance: Option<bool>,
	pub flags: Option<u64>,
	pub buttons: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Presence {
	pub user: Option<User>,
	pub user_id: Option<Snowflake>,
	pub guild_id: Option<Snowflake>,
	pub status: StatusKind,
	pub activities: LenientVec<Activity>,
	pub client_status: Option<ClientStatus>,
}
