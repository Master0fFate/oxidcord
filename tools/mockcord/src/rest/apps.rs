use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use protocol::{
	ApplicationCommandType, EditMessage, InteractionMetadata, InteractionType, MessageFlags,
	MessageReference, MessageType, Snowflake, User,
};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::json::message_id;
use crate::json::parse_id;
use crate::ops::Draft;
use crate::state::Shared;

pub(super) const NOTES_APP_ID: u64 = 990_000_000_000_000_001;
const APPLICATION_COMMAND: u64 = 2;
const COMPONENT: u64 = 3;
const AUTOCOMPLETE: u64 = 4;
const MODAL_SUBMIT: u64 = 5;
const TIMEOUT_REASON: u64 = 2;
const DEFERRED_DELAY: Duration = Duration::from_secs(2);
const CHAT_INPUT_COMMAND: u64 = 1;
const USER_COMMAND: u64 = 2;
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

fn command(id: u64, kind: u64, application_id: u64, name: &str, contexts: &[u8]) -> Value {
	json!({
		"id": id.to_string(),
		"type": kind,
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
		command(id + 1, MESSAGE_COMMAND, id, "Quote Message", &[0]),
		command(id + 2, MESSAGE_COMMAND, id, "Translate to English", &[0, 1, 2]),
		command(id + 6, USER_COMMAND, id, "Wave", &[0, 1, 2]),
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
		"application_commands": [
			command(NOTES_APP_ID + 1, MESSAGE_COMMAND, NOTES_APP_ID, "Save to Notes", &[0, 1, 2]),
			command(NOTES_APP_ID + 2, USER_COMMAND, NOTES_APP_ID, "Note About", &[0, 1, 2]),
		],
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

enum Reply {
	Success,
	Failure,
	Modal(Value),
}

fn invocation(shared: &Shared, id: u64, kind: InteractionType, name: Option<&str>) -> InteractionMetadata {
	InteractionMetadata {
		id: Snowflake(id),
		kind,
		name: name.map(str::to_owned),
		command_type: name.map(|_| ApplicationCommandType::ChatInput),
		user: shared
			.fixtures
			.users
			.get(&CURRENT_USER_ID)
			.cloned()
			.unwrap_or_default(),
		..InteractionMetadata::default()
	}
}

fn run_chat_command(
	shared: &Shared,
	channel_id: u64,
	interaction: u64,
	data: &Value,
) -> Result<Reply, ApiError> {
	let name = data["name"].as_str().unwrap_or_default();
	let options = &data["options"];
	let reply = match name {
		"roll" => roll_reply(options),
		"echo" => echo_reply(options),
		"note" => note_reply(options),
		_ => return Err(ApiError::invalid_body("Unknown command")),
	};
	let author = app_bot(shared).ok_or_else(ApiError::unknown_message)?;
	let mut draft = Draft::text(reply);
	draft.kind = Some(MessageType::ChatInputCommand);
	draft.interaction_metadata = Some(invocation(
		shared,
		interaction,
		InteractionType::ApplicationCommand,
		Some(name),
	));
	shared
		.post_message(channel_id, &author, draft)
		.map(|_| Reply::Success)
}

fn run_message_command(shared: &Shared, channel_id: u64, data: &Value) -> Result<Reply, ApiError> {
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
	shared
		.post_message(channel_id, &author, draft)
		.map(|_| Reply::Success)
}

fn run_user_command(shared: &Shared, channel_id: u64, data: &Value) -> Result<Reply, ApiError> {
	let target = parse_id(&data["target_id"]).unwrap_or_default();
	let reply = match data["name"].as_str().unwrap_or_default() {
		"Wave" => format!("Waved at <@{target}>"),
		_ => format!("Noted something about <@{target}>"),
	};
	let author = app_bot(shared).ok_or_else(ApiError::unknown_message)?;
	shared
		.post_message(channel_id, &author, Draft::text(reply))
		.map(|_| Reply::Success)
}

fn upgrade_modal(shared: &Shared, body: &Value) -> Value {
	let application = app_bot(shared).unwrap_or_default();
	json!({
		"channel_id": body["channel_id"],
		"custom_id": "upgrade_form",
		"title": "Upgrade",
		"application": { "id": application.id.to_string(), "name": application.username },
		"components": [
			{
				"type": 18,
				"id": 1,
				"label": "Version",
				"description": "Which release to install",
				"component": {
					"type": 4,
					"id": 2,
					"custom_id": "version",
					"style": 1,
					"placeholder": "0.5.9",
					"value": "0.5.9",
					"min_length": 1,
					"max_length": 20
				}
			},
			{
				"type": 18,
				"id": 3,
				"label": "Channel",
				"component": {
					"type": 21,
					"id": 4,
					"custom_id": "channel",
					"options": [
						{ "label": "Stable", "value": "stable", "default": true },
						{ "label": "Beta", "value": "beta" },
						{ "label": "Nightly", "value": "nightly" }
					]
				}
			},
			{
				"type": 18,
				"id": 5,
				"label": "Theme",
				"component": {
					"type": 3,
					"id": 6,
					"custom_id": "theme",
					"options": [
						{ "label": "Dark", "value": "dark" },
						{ "label": "Light", "value": "light" }
					]
				}
			},
			{
				"type": 18,
				"id": 7,
				"label": "Notes",
				"component": { "type": 4, "id": 8, "custom_id": "notes", "style": 2, "required": false, "max_length": 400 }
			},
			{
				"type": 18,
				"id": 9,
				"label": "Restart now",
				"component": { "type": 23, "id": 10, "custom_id": "restart", "default": true }
			}
		],
	})
}

struct Origin<'a> {
	interaction: u64,
	kind: InteractionType,
	session_id: Option<&'a str>,
	clicked: Option<u64>,
}

fn app_message(
	shared: &Shared,
	channel_id: u64,
	origin: Origin,
	flags: MessageFlags,
	content: &str,
) -> Result<Value, ApiError> {
	let author = app_bot(shared).ok_or_else(ApiError::unknown_message)?;
	let mut draft = Draft::text(content);
	draft.kind = Some(MessageType::Reply);
	draft.flags = flags;
	draft.only_for = origin
		.session_id
		.filter(|_| flags.contains(MessageFlags::EPHEMERAL))
		.map(str::to_owned);
	draft.interaction_metadata = Some(invocation(shared, origin.interaction, origin.kind, None));
	draft.reference = origin.clicked.map(|clicked| MessageReference {
		kind: None,
		message_id: Some(Snowflake(clicked)),
		channel_id: Some(Snowflake(channel_id)),
		guild_id: None,
		fail_if_not_exists: Some(false),
	});
	shared.post_message(channel_id, &author, draft)
}

fn defer_reply(shared: &Arc<Shared>, channel_id: u64, origin: Origin) -> Result<(), ApiError> {
	let loading = app_message(shared, channel_id, origin, MessageFlags::LOADING, "")?;
	let id = message_id(&loading);
	let author = app_bot(shared).map_or(0, |bot| bot.id.get());
	let shared = Arc::clone(shared);
	tokio::spawn(async move {
		tokio::time::sleep(DEFERRED_DELAY).await;
		let resolved = EditMessage {
			content: Some("Glad you like it".to_owned()),
			flags: Some(MessageFlags::empty()),
			..EditMessage::default()
		};
		let _ = shared.edit_as(channel_id, id, author, resolved);
	});
	Ok(())
}

fn use_component(shared: &Arc<Shared>, body: &Value, interaction: u64) -> Result<Reply, ApiError> {
	let channel_id = parse_id(&body["channel_id"]).unwrap_or_default();
	let origin = Origin {
		interaction,
		kind: InteractionType::MessageComponent,
		session_id: body["session_id"].as_str(),
		clicked: parse_id(&body["message_id"]),
	};
	match body["data"]["custom_id"].as_str() {
		Some("upgrade") => Ok(Reply::Modal(upgrade_modal(shared, body))),
		Some("later") => app_message(
			shared,
			channel_id,
			origin,
			MessageFlags::EPHEMERAL,
			"Reminder set for tomorrow",
		)
		.map(|_| Reply::Success),
		Some("love") => defer_reply(shared, channel_id, origin).map(|()| Reply::Success),
		Some("fail") => Ok(Reply::Failure),
		_ => Ok(Reply::Success),
	}
}

fn modal_field<'a>(components: &'a Value, custom_id: &str) -> &'a Value {
	components
		.as_array()
		.into_iter()
		.flatten()
		.map(|row| &row["component"])
		.find(|field| field["custom_id"].as_str() == Some(custom_id))
		.unwrap_or(&Value::Null)
}

fn submit_modal(shared: &Shared, body: &Value, interaction: u64) -> Result<Reply, ApiError> {
	let channel_id = parse_id(&body["channel_id"]).unwrap_or_default();
	let components = &body["data"]["components"];
	let version = modal_field(components, "version")["value"]
		.as_str()
		.unwrap_or("?");
	let channel = modal_field(components, "channel")["value"]
		.as_str()
		.unwrap_or("stable");
	let restart = modal_field(components, "restart")["value"]
		.as_bool()
		.unwrap_or(false);
	let content = format!(
		"Upgrading to {version} on {channel}{}",
		if restart { ", restarting after" } else { "" }
	);
	let origin = Origin {
		interaction,
		kind: InteractionType::ModalSubmit,
		session_id: body["session_id"].as_str(),
		clicked: None,
	};
	app_message(shared, channel_id, origin, MessageFlags::empty(), &content).map(|_| Reply::Success)
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

fn notify(shared: &Shared, body: &Value, name: &str, mut data: Value) {
	data["nonce"] = body["nonce"].clone();
	if let Some(session_id) = body["session_id"].as_str() {
		shared.hub.dispatch_to(session_id, name, &data);
	}
}

pub(super) fn interact(shared: &Arc<Shared>, body: &Value) -> Result<(), ApiError> {
	let channel_id = parse_id(&body["channel_id"]).unwrap_or_default();
	let data = &body["data"];
	let interaction = shared.store.lock().next_snowflake();
	let id = json!({ "id": interaction.to_string() });
	let reply = match (body["type"].as_u64(), data["type"].as_u64()) {
		(Some(AUTOCOMPLETE), _) => {
			complete(shared, body);
			return Ok(());
		}
		(Some(APPLICATION_COMMAND), Some(MESSAGE_COMMAND)) => {
			notify(shared, body, "INTERACTION_CREATE", id.clone());
			run_message_command(shared, channel_id, data)
		}
		(Some(APPLICATION_COMMAND), Some(USER_COMMAND)) => {
			notify(shared, body, "INTERACTION_CREATE", id.clone());
			run_user_command(shared, channel_id, data)
		}
		(Some(APPLICATION_COMMAND), Some(CHAT_INPUT_COMMAND)) => {
			notify(shared, body, "INTERACTION_CREATE", id.clone());
			run_chat_command(shared, channel_id, interaction, data)
		}
		(Some(COMPONENT), _) => {
			notify(shared, body, "INTERACTION_CREATE", id.clone());
			use_component(shared, body, interaction)
		}
		(Some(MODAL_SUBMIT), _) => {
			notify(shared, body, "INTERACTION_CREATE", id.clone());
			submit_modal(shared, body, interaction)
		}
		_ => return Ok(()),
	}?;
	match reply {
		Reply::Success => notify(shared, body, "INTERACTION_SUCCESS", id),
		Reply::Failure => notify(
			shared,
			body,
			"INTERACTION_FAILURE",
			json!({ "id": interaction.to_string(), "reason_code": TIMEOUT_REASON }),
		),
		Reply::Modal(mut modal) => {
			modal["id"] = id["id"].clone();
			notify(shared, body, "INTERACTION_MODAL_CREATE", modal);
		}
	}
	Ok(())
}
