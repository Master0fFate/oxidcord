use serde::{Deserialize, Serialize};

use crate::lenient::{LenientVec, int_enum, or_default};
use crate::snowflake::Snowflake;

int_enum! {
	pub enum AutoModTriggerType {
		Keyword = 1,
		Spam = 3,
		KeywordPreset = 4,
		MentionSpam = 5,
		MemberProfile = 6,
	}
}

int_enum! {
	pub enum AutoModEventType {
		MessageSend = 1,
		MemberUpdate = 2,
	}
}

int_enum! {
	pub enum AutoModActionType {
		BlockMessage = 1,
		SendAlert = 2,
		Timeout = 3,
		BlockMemberInteraction = 4,
	}
}

int_enum! {
	pub enum AutoModPreset {
		Profanity = 1,
		SexualContent = 2,
		Slurs = 3,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoModTriggerMetadata {
	#[serde(deserialize_with = "or_default")]
	pub keyword_filter: Vec<String>,
	#[serde(deserialize_with = "or_default")]
	pub regex_patterns: Vec<String>,
	pub presets: LenientVec<AutoModPreset>,
	#[serde(deserialize_with = "or_default")]
	pub allow_list: Vec<String>,
	pub mention_total_limit: Option<i64>,
	pub mention_raid_protection_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoModActionMetadata {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub channel_id: Option<Snowflake>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub duration_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub custom_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoModAction {
	#[serde(rename = "type")]
	pub kind: AutoModActionType,
	#[serde(deserialize_with = "or_default")]
	pub metadata: AutoModActionMetadata,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoModRule {
	pub id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub name: String,
	pub creator_id: Option<Snowflake>,
	pub event_type: AutoModEventType,
	pub trigger_type: AutoModTriggerType,
	#[serde(deserialize_with = "or_default")]
	pub trigger_metadata: AutoModTriggerMetadata,
	pub actions: LenientVec<AutoModAction>,
	pub enabled: bool,
	pub exempt_roles: LenientVec<Snowflake>,
	pub exempt_channels: LenientVec<Snowflake>,
}

impl AutoModRule {
	pub fn action(&self, kind: AutoModActionType) -> Option<&AutoModAction> {
		self.actions.iter().find(|action| action.kind == kind)
	}
}
