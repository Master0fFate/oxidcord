use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use protocol::{MessageReference, Snowflake, User};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::json::parse_id;
use crate::ops::Draft;
use crate::state::Shared;

const NOTES_APP_ID: u64 = 990_000_000_000_000_001;
const APPLICATION_COMMAND: u64 = 2;
const MESSAGE_COMMAND: u64 = 3;

fn app_bot(shared: &Shared) -> Option<User> {
	shared
		.fixtures
		.users
		.values()
		.find(|user| user.bot == Some(true))
		.cloned()
}

fn command(id: u64, application_id: u64, name: &str, contexts: &[u8]) -> Value {
	json!({
		"id": id.to_string(),
		"type": MESSAGE_COMMAND,
		"application_id": application_id.to_string(),
		"version": id.to_string(),
		"name": name,
		"description": "",
		"contexts": contexts,
		"default_member_permissions": null,
	})
}

fn bot_index(shared: &Shared) -> Value {
	let Some(bot) = app_bot(shared) else {
		return json!({ "applications": [], "application_commands": [] });
	};
	let id = bot.id.get();
	json!({
		"applications": [{ "id": id.to_string(), "name": bot.username, "bot_id": id.to_string(), "flags": "0" }],
		"application_commands": [
			command(id + 1, id, "Quote Message", &[0]),
			command(id + 2, id, "Translate to English", &[0, 1, 2]),
		],
	})
}

pub async fn guild_index(State(shared): State<Arc<Shared>>, Path(_): Path<u64>) -> Json<Value> {
	Json(bot_index(&shared))
}

pub async fn channel_index(State(shared): State<Arc<Shared>>, Path(_): Path<u64>) -> Json<Value> {
	Json(bot_index(&shared))
}

pub async fn user_index() -> Json<Value> {
	Json(json!({
		"applications": [{ "id": NOTES_APP_ID.to_string(), "name": "Pocket Notes", "flags": "0" }],
		"application_commands": [command(NOTES_APP_ID + 1, NOTES_APP_ID, "Save to Notes", &[0, 1, 2])],
	}))
}

pub(super) fn run_command(shared: &Shared, body: &Value) -> Result<(), ApiError> {
	let channel_id = parse_id(&body["channel_id"]).unwrap_or_default();
	let data = &body["data"];
	if data["type"].as_u64() != Some(MESSAGE_COMMAND) {
		return Err(ApiError::invalid_body("Only message commands are mocked"));
	}
	let target = parse_id(&data["target_id"]).unwrap_or_default();
	let content = shared
		.store
		.lock()
		.message(channel_id, target)
		.and_then(|message| message["content"].as_str().map(str::to_owned))
		.unwrap_or_default();
	let reply = match data["name"].as_str().unwrap_or_default() {
		"Quote Message" => format!("> {}", content.replace('\n', "\n> ")),
		"Translate to English" => format!("**English:** {content}"),
		_ => "Saved to your notes".to_owned(),
	};
	let author = app_bot(shared).ok_or_else(ApiError::unknown_message)?;
	let mut draft = Draft::text(reply);
	draft.reference = Some(MessageReference {
		kind: None,
		message_id: Some(Snowflake(target)),
		channel_id: Some(Snowflake(channel_id)),
		guild_id: None,
		fail_if_not_exists: Some(false),
	});
	shared.post_message(channel_id, &author, draft).map(drop)
}

pub(super) fn is_command(body: &Value) -> bool {
	body["type"].as_u64() == Some(APPLICATION_COMMAND)
}
