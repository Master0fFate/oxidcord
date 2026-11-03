use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use protocol::{MessageReference, MessageType, Snowflake, User};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::json::parse_id;
use crate::ops::Draft;
use crate::state::Shared;

pub(super) const NOTES_APP_ID: u64 = 990_000_000_000_000_001;
const APPLICATION_COMMAND: u64 = 2;
const AUTOCOMPLETE: u64 = 4;
const CHAT_INPUT_COMMAND: u64 = 1;
const MESSAGE_COMMAND: u64 = 3;
const SUB_COMMAND_OPTION: u64 = 1;
const STRING_OPTION: u64 = 3;
const INTEGER_OPTION: u64 = 4;
const USER_OPTION: u64 = 6;
const PHRASES: [&str; 5] = [
	"hello there",
	"good morning",
	"good night",
	"thank you",
	"see you soon",
];

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

fn chat_command(id: u64, application_id: u64, name: &str, description: &str, options: Value) -> Value {
	json!({
		"id": id.to_string(),
		"type": CHAT_INPUT_COMMAND,
		"application_id": application_id.to_string(),
		"version": id.to_string(),
		"name": name,
		"description": description,
		"contexts": [0, 1, 2],
		"default_member_permissions": null,
		"options": options,
	})
}

fn option(kind: u64, name: &str, description: &str, required: bool) -> Value {
	json!({ "type": kind, "name": name, "description": description, "required": required })
}

fn chat_commands(id: u64) -> Vec<Value> {
	let mut sides = option(INTEGER_OPTION, "sides", "Number of sides", true);
	sides["min_value"] = json!(2);
	sides["max_value"] = json!(100);
	let mut text = option(STRING_OPTION, "text", "Phrase to repeat", true);
	text["autocomplete"] = json!(true);
	let mut style = option(STRING_OPTION, "style", "How to say it", false);
	style["choices"] = json!([
		{ "name": "Plain", "value": "plain" },
		{ "name": "Loud", "value": "loud" },
		{ "name": "Quiet", "value": "quiet" },
	]);
	let mut add = option(SUB_COMMAND_OPTION, "add", "Save a note", false);
	add["options"] = json!([option(STRING_OPTION, "text", "Note body", true)]);
	vec![
		chat_command(id + 3, id, "roll", "Roll a die", json!([sides])),
		chat_command(
			id + 4,
			id,
			"echo",
			"Repeat a phrase",
			json!([text, style, option(USER_OPTION, "user", "Who to greet", false)]),
		),
		chat_command(
			id + 5,
			id,
			"note",
			"Keep short notes",
			json!([add, option(SUB_COMMAND_OPTION, "list", "Show notes", false)]),
		),
	]
}

fn bot_index(shared: &Shared) -> Value {
	let Some(bot) = app_bot(shared) else {
		return json!({ "applications": [], "application_commands": [] });
	};
	let id = bot.id.get();
	let mut commands = vec![
		command(id + 1, id, "Quote Message", &[0]),
		command(id + 2, id, "Translate to English", &[0, 1, 2]),
	];
	commands.extend(chat_commands(id));
	json!({
		"applications": [{ "id": id.to_string(), "name": bot.username, "bot_id": id.to_string(), "flags": "0" }],
		"application_commands": commands,
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

fn option_value<'a>(options: &'a Value, name: &str) -> Option<&'a Value> {
	options
		.as_array()?
		.iter()
		.find(|option| option["name"].as_str() == Some(name))
		.map(|option| &option["value"])
}

fn roll_reply(options: &Value) -> String {
	let sides = option_value(options, "sides")
		.and_then(Value::as_u64)
		.filter(|sides| *sides > 0)
		.unwrap_or(6);
	let nanos = u64::from(chrono::Utc::now().timestamp_subsec_nanos());
	format!("Rolled {} on a d{sides}", nanos % sides + 1)
}

fn echo_reply(options: &Value) -> String {
	let text = option_value(options, "text")
		.and_then(Value::as_str)
		.unwrap_or_default();
	let text = match option_value(options, "style").and_then(Value::as_str) {
		Some("loud") => text.to_uppercase(),
		Some("quiet") => format!("-# {text}"),
		_ => text.to_owned(),
	};
	match option_value(options, "user").and_then(Value::as_str) {
		Some(user) => format!("<@{user}> {text}"),
		None => text,
	}
}

fn note_reply(options: &Value) -> String {
	let sub = &options[0];
	match sub["name"].as_str() {
		Some("add") => format!(
			"Noted: {}",
			option_value(&sub["options"], "text")
				.and_then(Value::as_str)
				.unwrap_or_default()
		),
		_ => "No notes yet".to_owned(),
	}
}

fn run_chat_command(shared: &Shared, channel_id: u64, data: &Value) -> Result<(), ApiError> {
	let name = data["name"].as_str().unwrap_or_default();
	let options = &data["options"];
	let reply = match name {
		"roll" => roll_reply(options),
		"echo" => echo_reply(options),
		"note" => note_reply(options),
		_ => return Err(ApiError::invalid_body("Unknown command")),
	};
	let author = app_bot(shared).ok_or_else(ApiError::unknown_message)?;
	let interaction = shared.store.lock().next_snowflake();
	let mut draft = Draft::text(reply);
	draft.kind = Some(MessageType::ChatInputCommand);
	draft.interaction_metadata = json!({
		"id": interaction.to_string(),
		"type": APPLICATION_COMMAND,
		"name": name,
		"user": { "id": CURRENT_USER_ID.to_string() },
	});
	shared.post_message(channel_id, &author, draft).map(drop)
}

fn run_message_command(shared: &Shared, channel_id: u64, data: &Value) -> Result<(), ApiError> {
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

fn complete(shared: &Shared, body: &Value) {
	let typed = body["data"]["options"]
		.as_array()
		.and_then(|options| options.iter().find(|option| option["focused"] == true))
		.and_then(|option| option["value"].as_str())
		.unwrap_or_default()
		.to_lowercase();
	let choices: Vec<Value> = PHRASES
		.iter()
		.filter(|phrase| phrase.contains(&typed))
		.map(|phrase| json!({ "name": phrase, "value": phrase }))
		.collect();
	if let Some(session_id) = body["session_id"].as_str() {
		shared.hub.dispatch_to(
			session_id,
			"APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE",
			&json!({ "nonce": body["nonce"], "choices": choices }),
		);
	}
}

pub(super) fn interact(shared: &Shared, body: &Value) -> Result<(), ApiError> {
	let channel_id = parse_id(&body["channel_id"]).unwrap_or_default();
	let data = &body["data"];
	match (body["type"].as_u64(), data["type"].as_u64()) {
		(Some(APPLICATION_COMMAND), Some(MESSAGE_COMMAND)) => run_message_command(shared, channel_id, data),
		(Some(APPLICATION_COMMAND), Some(CHAT_INPUT_COMMAND)) => run_chat_command(shared, channel_id, data),
		(Some(AUTOCOMPLETE), _) => {
			complete(shared, body);
			Ok(())
		}
		_ => Ok(()),
	}
}
