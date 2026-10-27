use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::component::ComponentType;
use crate::message::{MessageFlags, MessageReference};
use crate::snowflake::Snowflake;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AttachmentDescriptor {
	pub id: Snowflake,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub filename: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub title: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub description: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub content_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub duration_secs: Option<f64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub waveform: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AllowedMentions {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub parse: Option<Vec<String>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub roles: Option<Vec<Snowflake>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub users: Option<Vec<Snowflake>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub replied_user: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComponentInteraction {
	#[serde(rename = "type")]
	pub kind: u8,
	pub nonce: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub guild_id: Option<Snowflake>,
	pub channel_id: Snowflake,
	pub message_flags: MessageFlags,
	pub message_id: Snowflake,
	pub application_id: Snowflake,
	pub session_id: String,
	pub data: ComponentInteractionData,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComponentInteractionData {
	pub component_type: ComponentType,
	pub custom_id: String,
	#[serde(rename = "type", skip_serializing_if = "Option::is_none")]
	pub select_type: Option<ComponentType>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub values: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CreateMessage {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub content: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub nonce: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tts: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub message_reference: Option<MessageReference>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub allowed_mentions: Option<AllowedMentions>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub flags: Option<MessageFlags>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub sticker_ids: Vec<Snowflake>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub attachments: Vec<AttachmentDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EditMessage {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub content: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub flags: Option<MessageFlags>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub allowed_mentions: Option<AllowedMentions>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub attachments: Option<Vec<AttachmentDescriptor>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessagesQuery {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub around: Option<Snowflake>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub before: Option<Snowflake>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub after: Option<Snowflake>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub limit: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitBody {
	pub message: String,
	pub retry_after: f64,
	pub global: bool,
	pub code: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiError {
	pub code: i64,
	pub message: String,
	pub errors: Option<Value>,
}
