use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::lenient::{LenientVec, int_enum};
use crate::snowflake::Snowflake;

int_enum! {
	pub enum ApplicationCommandType {
		ChatInput = 1,
		User = 2,
		Message = 3,
		PrimaryEntryPoint = 4,
	}
}

int_enum! {
	pub enum InteractionContext {
		Guild = 0,
		BotDm = 1,
		PrivateChannel = 2,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApplicationCommand {
	pub id: Snowflake,
	#[serde(rename = "type")]
	pub kind: ApplicationCommandType,
	pub application_id: Snowflake,
	pub name: String,
	pub description: Option<String>,
	pub version: Option<Snowflake>,
	pub default_member_permissions: Option<String>,
	pub contexts: Option<Vec<InteractionContext>>,
	pub nsfw: bool,
	#[serde(flatten)]
	pub rest: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandApplication {
	pub id: Snowflake,
	pub name: String,
	pub description: Option<String>,
	pub icon: Option<String>,
	pub bot_id: Option<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandIndex {
	pub applications: LenientVec<CommandApplication>,
	pub application_commands: LenientVec<ApplicationCommand>,
}
