use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::lenient::{LenientVec, str_enum};
use crate::snowflake::Snowflake;

str_enum! {
	pub enum FormFieldKind {
		Terms = "TERMS",
		TextInput = "TEXT_INPUT",
		Paragraph = "PARAGRAPH",
		MultipleChoice = "MULTIPLE_CHOICE",
	}
}

str_enum! {
	pub enum JoinRequestStatus {
		Started = "STARTED",
		Submitted = "SUBMITTED",
		Rejected = "REJECTED",
		Approved = "APPROVED",
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FormField {
	pub field_type: FormFieldKind,
	pub label: String,
	#[serde(skip_serializing_if = "is_empty")]
	pub values: LenientVec<String>,
	#[serde(skip_serializing_if = "is_empty")]
	pub choices: LenientVec<String>,
	pub required: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub description: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub placeholder: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub response: Option<Value>,
}

fn is_empty(items: &LenientVec<String>) -> bool {
	items.is_empty()
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemberVerification {
	pub version: Option<String>,
	pub description: Option<String>,
	pub form_fields: LenientVec<FormField>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct InviteAccepted {
	pub guild_id: Option<Snowflake>,
	pub show_verification_form: bool,
	pub new_member: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct JoinRequest {
	pub application_status: Option<JoinRequestStatus>,
	pub user_id: Option<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct JoinRequestChange {
	pub guild_id: Snowflake,
	pub status: Option<JoinRequestStatus>,
	pub request: Option<JoinRequest>,
}

impl JoinRequestChange {
	pub fn user_id(&self) -> Option<Snowflake> {
		self.request.as_ref()?.user_id
	}

	pub fn current_status(&self) -> Option<JoinRequestStatus> {
		self.status
			.clone()
			.or_else(|| self.request.as_ref()?.application_status.clone())
	}
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct JoinRequestDelete {
	pub guild_id: Snowflake,
	pub user_id: Option<Snowflake>,
}
