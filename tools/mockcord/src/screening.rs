use protocol::Snowflake;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::{CURRENT_USER_ID, GuildFixture, MEMBER_GATE_FEATURE};
use crate::json::to_json;
use crate::state::{Shared, Store};

pub const APPLY_CODE: &str = "mockapply";
const APPLY_GUILD_ID: u64 = 910_000_000_000_000_001;
const APPLY_GUILD_NAME: &str = "Applicant Hall";
const APPLY_DESCRIPTION: &str = "A closed community that reviews every request.";
const STATUS_SUBMITTED: &str = "SUBMITTED";
const STATUS_REJECTED: &str = "REJECTED";
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

fn missing_answers(body: &Value) -> bool {
	verification_form(None)["form_fields"]
		.as_array()
		.into_iter()
		.flatten()
		.enumerate()
		.any(|(index, field)| field["required"] == json!(true) && !answered(body, index))
}

fn apply_guild() -> Value {
	json!({
		"id": APPLY_GUILD_ID.to_string(),
		"name": APPLY_GUILD_NAME,
		"icon": null,
		"splash": null,
		"banner": null,
		"description": APPLY_DESCRIPTION,
		"features": ["COMMUNITY", MEMBER_GATE_FEATURE, "MEMBER_VERIFICATION_MANUAL_APPROVAL"],
		"verification_level": 1,
		"vanity_url_code": null,
		"nsfw_level": 0,
		"premium_subscription_count": 0,
	})
}

fn join_request(status: &str) -> Value {
	json!({
		"id": APPLY_GUILD_ID.to_string(),
		"guild_id": APPLY_GUILD_ID.to_string(),
		"user_id": CURRENT_USER_ID.to_string(),
		"application_status": status,
	})
}

impl Shared {
	pub fn application_invite(&self, code: &str, accepted: bool) -> Option<Value> {
		(code == APPLY_CODE).then(|| {
			let mut invite = json!({
				"type": 0,
				"code": code,
				"expires_at": null,
				"guild_id": APPLY_GUILD_ID.to_string(),
				"guild": apply_guild(),
				"channel": null,
				"approximate_member_count": 128,
				"approximate_presence_count": 12,
			});
			if accepted {
				invite["new_member"] = json!(false);
				invite["show_verification_form"] = json!(true);
			}
			invite
		})
	}

	pub fn application_requests(&self, store: &Store) -> Vec<Value> {
		store.application.map(join_request).into_iter().collect()
	}

	pub fn join_request_guilds_json(&self) -> Value {
		let applied = self.store.lock().application.is_some();
		Value::Array(applied.then(apply_guild).into_iter().collect())
	}

	pub fn delete_join_request(&self, guild_id: u64) -> Result<(), ApiError> {
		if guild_id != APPLY_GUILD_ID {
			return Err(ApiError::unknown_guild());
		}
		self.store.lock().application = None;
		self.hub.broadcast(
			"GUILD_JOIN_REQUEST_DELETE",
			&json!({ "guild_id": guild_id.to_string(), "user_id": CURRENT_USER_ID.to_string() }),
		);
		Ok(())
	}

	pub fn reject_application(&self) {
		self.store.lock().application = Some(STATUS_REJECTED);
		self.broadcast_application(STATUS_REJECTED, "GUILD_JOIN_REQUEST_UPDATE");
	}

	fn broadcast_application(&self, status: &str, event: &str) {
		self.hub.broadcast(
			event,
			&json!({
				"guild_id": APPLY_GUILD_ID.to_string(),
				"status": status,
				"request": join_request(status),
			}),
		);
	}

	fn submit_application(&self, body: &Value) -> Result<Value, ApiError> {
		if missing_answers(body) {
			return Err(ApiError::invalid_body("Required form fields are missing"));
		}
		self.store.lock().application = Some(STATUS_SUBMITTED);
		self.broadcast_application(STATUS_SUBMITTED, "GUILD_JOIN_REQUEST_CREATE");
		Ok(join_request(STATUS_SUBMITTED))
	}

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
		if guild_id == APPLY_GUILD_ID {
			return Ok(verification_form(Some(APPLY_DESCRIPTION)));
		}
		let guild = self.gated_guild(guild_id)?;
		Ok(verification_form(guild.properties.description.as_deref()))
	}

	pub fn submit_join_request(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		if guild_id == APPLY_GUILD_ID {
			return self.submit_application(body);
		}
		let guild = self.gated_guild(guild_id)?;
		if missing_answers(body) {
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
