use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::Utc;
use serde_json::{Value, json};

use crate::error::{ApiError, ApiResult};
use crate::json::{parse_id, to_json};
use crate::state::Shared;

type Mock = State<Arc<Shared>>;
type NoContent = Result<StatusCode, ApiError>;

const AUDIT_REASON: &str = "x-audit-log-reason";
const DEFAULT_REACTION_LIMIT: usize = 25;

fn audit_reason(headers: &HeaderMap) -> Option<String> {
	let raw = headers.get(AUDIT_REASON)?.to_str().ok()?;
	let bytes = raw.as_bytes();
	let mut decoded = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		let escaped = (bytes[index] == b'%')
			.then(|| raw.get(index + 1..index + 3))
			.flatten()
			.and_then(|hex| u8::from_str_radix(hex, 16).ok());
		match escaped {
			Some(byte) => {
				decoded.push(byte);
				index += 3;
			}
			None => {
				decoded.push(bytes[index]);
				index += 1;
			}
		}
	}
	String::from_utf8(decoded).ok()
}

fn settings_scope(scope: &str) -> u64 {
	scope.parse().unwrap_or(0)
}

pub async fn add_friend(State(shared): Mock, Json(body): Json<Value>) -> NoContent {
	let username = body["username"]
		.as_str()
		.ok_or_else(|| ApiError::invalid_body("username is required"))?;
	shared.add_relationship_by_username(username)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn put_relationship(
	State(shared): Mock,
	Path(id): Path<u64>,
	body: Option<Json<Value>>,
) -> NoContent {
	let kind = body.and_then(|Json(body)| body["type"].as_i64());
	shared.put_relationship(id, kind)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_relationship(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.delete_relationship(id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn patch_me(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	Ok(Json(shared.patch_current_user(&body)))
}

pub async fn patch_profile(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	Ok(Json(shared.patch_current_profile(&body)))
}

pub async fn patch_settings(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	shared.hub.broadcast("USER_SETTINGS_UPDATE", &body);
	Ok(Json(body))
}

pub async fn patch_guild_settings(
	State(shared): Mock,
	Path(scope): Path<String>,
	Json(body): Json<Value>,
) -> ApiResult {
	Ok(Json(shared.patch_guild_settings(settings_scope(&scope), &body)))
}

pub async fn leave_guild(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.leave_guild(id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn ack_bulk(State(shared): Mock, Json(body): Json<Value>) -> NoContent {
	for entry in body["read_states"].as_array().into_iter().flatten() {
		if let (Some(channel_id), Some(message_id)) =
			(parse_id(&entry["channel_id"]), parse_id(&entry["message_id"]))
		{
			shared.ack(channel_id, message_id, None)?;
		}
	}
	Ok(StatusCode::NO_CONTENT)
}

pub async fn accept_invite(State(shared): Mock, Path(code): Path<String>) -> ApiResult {
	shared
		.invite_json(&code)
		.map(Json)
		.ok_or_else(ApiError::unknown_invite)
}

pub async fn create_guild(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	shared.create_guild(&body).map(Json)
}

pub async fn auth_sessions(State(shared): Mock) -> Json<Value> {
	let user_id = shared.store.lock().current_user.id.to_string();
	Json(json!({
		"user_sessions": [{
			"id_hash": format!("mock-session-{user_id}"),
			"approx_last_used_time": Utc::now().to_rfc3339(),
			"client_info": { "os": "windows", "platform": "desktop", "location": "Mock" },
		}]
	}))
}

pub async fn logout_sessions(Json(_body): Json<Value>) -> StatusCode {
	StatusCode::NO_CONTENT
}

pub async fn delete_invite(State(shared): Mock, Path(code): Path<String>) -> ApiResult {
	shared.delete_invite(&code).map(Json)
}

pub async fn create_invite(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.create_invite(id, &body).map(Json)
}

pub async fn guild_invites(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.guild_invites_json(id).map(Json)
}

#[derive(serde::Deserialize)]
pub struct AuditParams {
	before: Option<u64>,
	action_type: Option<i64>,
}

pub async fn audit_log(
	State(shared): Mock,
	Path(id): Path<u64>,
	Query(params): Query<AuditParams>,
) -> ApiResult {
	shared
		.audit_log_json(id, params.before, params.action_type)
		.map(Json)
}

pub async fn bans(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.bans_json(id).map(Json)
}

pub async fn active_threads(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	let guild_id = protocol::Snowflake(id);
	Ok(Json(shared.threads_json(|thread| {
		thread.guild_id == Some(guild_id)
			&& thread
				.thread_metadata
				.as_ref()
				.is_none_or(|metadata| !metadata.archived)
	})))
}

pub async fn archived_threads(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	let parent_id = protocol::Snowflake(id);
	Ok(Json(shared.threads_json(|thread| {
		thread.parent_id == Some(parent_id)
			&& thread
				.thread_metadata
				.as_ref()
				.is_some_and(|metadata| metadata.archived)
	})))
}

pub async fn modify_guild(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.modify_guild(id, &body).map(Json)
}

pub async fn create_channel(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.create_channel(id, &body).map(Json)
}

pub async fn create_role(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.create_role(id, &body).map(Json)
}

pub async fn reorder_roles(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.reorder_roles(id, &body).map(Json)
}

pub async fn modify_role(
	State(shared): Mock,
	Path((id, role_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared.modify_role(id, role_id, &body).map(Json)
}

pub async fn delete_role(State(shared): Mock, Path((id, role_id)): Path<(u64, u64)>) -> NoContent {
	shared.delete_role(id, role_id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn create_emoji(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.create_emoji(id, &body).map(Json)
}

pub async fn delete_emoji(State(shared): Mock, Path((id, emoji_id)): Path<(u64, u64)>) -> NoContent {
	shared.delete_emoji(id, emoji_id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn modify_member(
	State(shared): Mock,
	Path((id, user_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared.modify_member(id, user_id, &body).map(Json)
}

pub async fn modify_current_member(
	State(shared): Mock,
	Path(id): Path<u64>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared
		.modify_member(id, crate::fixtures::CURRENT_USER_ID, &body)
		.map(Json)
}

pub async fn kick(
	State(shared): Mock,
	Path((id, user_id)): Path<(u64, u64)>,
	headers: HeaderMap,
) -> NoContent {
	shared.drop_member(id, user_id, false, audit_reason(&headers).as_deref())?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn ban(
	State(shared): Mock,
	Path((id, user_id)): Path<(u64, u64)>,
	headers: HeaderMap,
) -> NoContent {
	shared.drop_member(id, user_id, true, audit_reason(&headers).as_deref())?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn unban(State(shared): Mock, Path((id, user_id)): Path<(u64, u64)>) -> NoContent {
	shared.unban(id, user_id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn add_member_role(
	State(shared): Mock,
	Path((id, user_id, role_id)): Path<(u64, u64, u64)>,
) -> NoContent {
	shared.set_member_role(id, user_id, role_id, true)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member_role(
	State(shared): Mock,
	Path((id, user_id, role_id)): Path<(u64, u64, u64)>,
) -> NoContent {
	shared.set_member_role(id, user_id, role_id, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn modify_channel(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.modify_channel(id, &body).map(Json)
}

pub async fn delete_channel(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.delete_channel(id).map(Json)
}

pub async fn put_overwrite(
	State(shared): Mock,
	Path((id, target_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> NoContent {
	shared.set_overwrite(id, target_id, Some(&body))?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_overwrite(State(shared): Mock, Path((id, target_id)): Path<(u64, u64)>) -> NoContent {
	shared.set_overwrite(id, target_id, None)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn join_thread(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.thread_membership(id, true)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn leave_thread(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.thread_membership(id, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn add_recipient(State(shared): Mock, Path((id, user_id)): Path<(u64, u64)>) -> NoContent {
	shared.group_recipient(id, user_id, true)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_recipient(State(shared): Mock, Path((id, user_id)): Path<(u64, u64)>) -> NoContent {
	shared.group_recipient(id, user_id, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn bulk_delete(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> NoContent {
	let ids: Vec<u64> = body["messages"]
		.as_array()
		.map(|ids| ids.iter().filter_map(parse_id).collect())
		.unwrap_or_default();
	shared.bulk_delete(id, &ids)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn pin(State(shared): Mock, Path((id, message_id)): Path<(u64, u64)>) -> NoContent {
	shared.set_pin(id, message_id, true)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn unpin(State(shared): Mock, Path((id, message_id)): Path<(u64, u64)>) -> NoContent {
	shared.set_pin(id, message_id, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn vote(
	State(shared): Mock,
	Path((id, message_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> NoContent {
	let answers: Vec<i64> = body["answer_ids"]
		.as_array()
		.map(|ids| ids.iter().filter_map(Value::as_i64).collect())
		.unwrap_or_default();
	shared.vote_poll(id, message_id, &answers, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn expire_poll(State(shared): Mock, Path((id, message_id)): Path<(u64, u64)>) -> ApiResult {
	shared.vote_poll(id, message_id, &[], true).map(Json)
}

pub async fn reactions(
	State(shared): Mock,
	Path((id, message_id, emoji)): Path<(u64, u64, String)>,
	Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
	let limit = params
		.get("limit")
		.and_then(|limit| limit.parse().ok())
		.unwrap_or(DEFAULT_REACTION_LIMIT);
	shared.reaction_users(id, message_id, &emoji, limit).map(Json)
}

pub fn profile_extras(shared: &Shared, user_id: u64, guild_id: Option<u64>) -> Value {
	let fixtures = &shared.fixtures;
	let user = fixtures.users.get(&user_id);
	let store = shared.store.lock();
	let friends: Vec<Value> = store
		.relationships
		.values()
		.filter(|relationship| relationship.id.0 != user_id)
		.filter_map(|relationship| fixtures.users.get(&relationship.id.0))
		.take(3)
		.map(to_json)
		.collect();
	let badges = if user_id.is_multiple_of(2) {
		json!([{ "id": "premium", "description": "Subscriber", "icon": "2ba85e8026a8614b640c2837bcdfe21b" }])
	} else {
		json!([])
	};
	json!({
		"mutual_friends": friends,
		"badges": badges,
		"connected_accounts": user.map_or_else(Vec::new, |user| {
			vec![json!({ "type": "github", "id": user.id.to_string(), "name": user.username, "verified": true })]
		}),
		"guild_member": guild_id.and_then(|guild_id| {
			let guild = fixtures.guild(guild_id)?;
			store.member_now(guild, user_id).map(|member| to_json(&member))
		}),
	})
}
