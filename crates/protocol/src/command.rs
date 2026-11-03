use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::channel::ChannelType;
use crate::component::Component;
use crate::lenient::{LenientVec, int_enum};
use crate::message::{MessageApplication, Nonce};
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

int_enum! {
	pub enum CommandOptionType {
		SubCommand = 1,
		SubCommandGroup = 2,
		String = 3,
		Integer = 4,
		Boolean = 5,
		User = 6,
		Channel = 7,
		Role = 8,
		Mentionable = 9,
		Number = 10,
		Attachment = 11,
	}
}

impl CommandOptionType {
	pub fn is_group(self) -> bool {
		matches!(self, Self::SubCommand | Self::SubCommandGroup)
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandChoice {
	pub name: String,
	pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApplicationCommandOption {
	#[serde(rename = "type")]
	pub kind: CommandOptionType,
	pub name: String,
	pub description: Option<String>,
	pub required: bool,
	pub choices: LenientVec<CommandChoice>,
	pub options: LenientVec<ApplicationCommandOption>,
	pub channel_types: Vec<ChannelType>,
	pub min_value: Option<Number>,
	pub max_value: Option<Number>,
	pub min_length: Option<u32>,
	pub max_length: Option<u32>,
	pub autocomplete: bool,
	#[serde(flatten)]
	pub rest: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InteractionOption {
	#[serde(rename = "type")]
	pub kind: CommandOptionType,
	pub name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub value: Option<Value>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub options: Vec<InteractionOption>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub focused: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CommandAutocomplete {
	pub nonce: Option<Nonce>,
	pub choices: LenientVec<CommandChoice>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct InteractionAck {
	pub id: Snowflake,
	pub nonce: Option<Nonce>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct InteractionFailure {
	pub id: Snowflake,
	pub nonce: Option<Nonce>,
	pub reason_code: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct InteractionModal {
	pub id: Snowflake,
	pub nonce: Option<Nonce>,
	pub channel_id: Snowflake,
	pub custom_id: String,
	pub title: String,
	pub application: MessageApplication,
	pub components: LenientVec<Component>,
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
	pub options: LenientVec<ApplicationCommandOption>,
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
