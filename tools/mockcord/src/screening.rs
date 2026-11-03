use protocol::Snowflake;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::{CURRENT_USER_ID, GuildFixture, MEMBER_GATE_FEATURE};
use crate::json::to_json;
use crate::state::Shared;

const FORM_VERSION: &str = "2026-01-01T00:00:00.000000+00:00";
const STATUS_APPROVED: &str = "APPROVED";

fn verification_form(description: Option<&str>) -> Value {
	json!({
		"version": FORM_VERSION,
		"description": description,
		"form_fields": [
			{
				"field_type": "TERMS",
				"label": "Server Rules",
				"values": ["Be kind to everyone.", "No spam or self promotion.", "Keep topics in the right channel."],
				"required": true,
			},
			{
				"field_type": "MULTIPLE_CHOICE",
				"label": "How did you find us?",
				"choices": ["A friend", "Search", "Social media"],
				"required": false,
			},
			{
				"field_type": "TEXT_INPUT",
				"label": "Why do you want to join?",
				"placeholder": "Tell us a bit about yourself",
				"required": false,
			},
		],
	})
}

fn answered(body: &Value, index: usize) -> bool {
	!matches!(
		body["form_fields"][index]["response"],
		Value::Null | Value::Bool(false)
	)
}

impl Shared {
	fn gated_guild(&self, guild_id: u64) -> Result<&GuildFixture, ApiError> {
		self.fixtures
			.guild(guild_id)
			.filter(|guild| {
				guild
					.properties
					.features
					.iter()
					.any(|feature| feature == MEMBER_GATE_FEATURE)
			})
			.ok_or_else(ApiError::unknown_guild)
	}

	pub fn member_verification_json(&self, guild_id: u64) -> Result<Value, ApiError> {
		let guild = self.gated_guild(guild_id)?;
		Ok(verification_form(guild.properties.description.as_deref()))
	}

	pub fn submit_join_request(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		let guild = self.gated_guild(guild_id)?;
		let form = verification_form(None);
		let missing = form["form_fields"]
			.as_array()
			.into_iter()
			.flatten()
			.enumerate()
			.any(|(index, field)| field["required"] == json!(true) && !answered(body, index));
		if missing {
			return Err(ApiError::invalid_body("Required form fields are missing"));
		}
		let mut store = self.store.lock();
		let mut member = store
			.member_now(guild, CURRENT_USER_ID)
			.filter(|member| member.pending == Some(true))
			.ok_or_else(|| ApiError::invalid_body("No pending join request"))?;
		member.pending = Some(false);
		member.guild_id = Some(Snowflake(guild_id));
		let member_event = to_json(&member);
		store.member_edits.insert((guild_id, CURRENT_USER_ID), member);
		drop(store);
		let request = json!({
			"id": guild_id.to_string(),
			"guild_id": guild_id.to_string(),
			"user_id": CURRENT_USER_ID.to_string(),
			"application_status": STATUS_APPROVED,
		});
		self.hub.broadcast(
			"GUILD_JOIN_REQUEST_UPDATE",
			&json!({
				"guild_id": guild_id.to_string(),
				"status": STATUS_APPROVED,
				"request": request,
			}),
		);
		self.hub.broadcast("GUILD_MEMBER_UPDATE", &member_event);
		Ok(request)
	}
}
