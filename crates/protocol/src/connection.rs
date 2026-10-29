use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectionToken(String);

impl ConnectionToken {
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl fmt::Debug for ConnectionToken {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("ConnectionToken(..)")
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
	pub id: String,
	#[serde(rename = "type")]
	pub kind: String,
	pub name: String,
	pub verified: bool,
	pub show_activity: bool,
	pub friend_sync: bool,
	pub visibility: i64,
	#[serde(skip_serializing)]
	pub access_token: Option<ConnectionToken>,
}

impl Connection {
	pub const SPOTIFY: &str = "spotify";

	pub fn is_spotify(&self) -> bool {
		self.kind == Self::SPOTIFY
	}
}
