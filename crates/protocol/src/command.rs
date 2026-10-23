use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::snowflake::Snowflake;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApplicationCommand {
	pub id: Snowflake,
	#[serde(rename = "type")]
	pub kind: Option<i64>,
	pub application_id: Option<Snowflake>,
	pub name: String,
	pub description: Option<String>,
	pub options: Value,
}
