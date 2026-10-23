use serde::{Deserialize, Serialize};

use crate::lenient::present;
use crate::permissions::Permissions;
use crate::snowflake::Snowflake;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleColors {
	pub primary_color: Option<i64>,
	pub secondary_color: Option<i64>,
	pub tertiary_color: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RoleTags {
	pub bot_id: Option<Snowflake>,
	pub integration_id: Option<Snowflake>,
	#[serde(deserialize_with = "present")]
	pub premium_subscriber: bool,
	pub subscription_listing_id: Option<Snowflake>,
	#[serde(deserialize_with = "present")]
	pub available_for_purchase: bool,
	#[serde(deserialize_with = "present")]
	pub guild_connections: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Role {
	pub id: Snowflake,
	pub name: String,
	pub color: i64,
	pub colors: Option<RoleColors>,
	pub hoist: bool,
	pub icon: Option<String>,
	pub unicode_emoji: Option<String>,
	pub position: i64,
	pub permissions: Permissions,
	pub managed: bool,
	pub mentionable: bool,
	pub tags: Option<RoleTags>,
	pub flags: Option<u64>,
}
