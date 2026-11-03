use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use protocol::{Channel, ChannelType, decode_settings};
use serde_json::{Value, json};

use crate::error::{ApiError, ApiResult};
use crate::json::{parse_id, to_json};
use crate::ready::user_settings_proto;
use crate::state::{Shared, Store};

type Mock = State<Arc<Shared>>;
type NoContent = Result<StatusCode, ApiError>;

const AUDIT_REASON: &str = "x-audit-log-reason";
const MFA_AUTHORIZATION: &str = "x-discord-mfa-authorization";
const DEFAULT_REACTION_LIMIT: usize = 25;
const DEFAULT_ARCHIVED_LIMIT: usize = 50;

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
	shared.simulate_latency().await;
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

pub async fn ignore_user(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.set_ignored(id, true)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn unignore_user(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.set_ignored(id, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn patch_relationship(
	State(shared): Mock,
	Path(id): Path<u64>,
	Json(body): Json<Value>,
) -> NoContent {
	let nickname = body["nickname"]
		.as_str()
		.filter(|nickname| !nickname.is_empty())
		.map(str::to_owned);
	shared.set_friend_nickname(id, nickname)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn incoming_request(State(shared): Mock) -> NoContent {
	shared.incoming_request()?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn get_note(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	Ok(Json(shared.note(id)?))
}

pub async fn patch_connection(
	State(shared): Mock,
	Path((kind, id)): Path<(String, String)>,
	Json(body): Json<Value>,
) -> ApiResult {
	let updated = {
		let mut store = shared.store.lock();
		let connection = store
			.connections
			.iter_mut()
			.find(|connection| connection["type"] == kind && connection["id"] == id)
			.ok_or_else(ApiError::not_found)?;
		for key in [
			"visibility",
			"show_activity",
			"friend_sync",
			"metadata_visibility",
		] {
			if !body[key].is_null() {
				connection[key] = body[key].clone();
			}
		}
		connection.clone()
	};
	shared.hub.broadcast("USER_CONNECTIONS_UPDATE", &json!({}));
	Ok(Json(updated))
}

pub async fn delete_connection(State(shared): Mock, Path((kind, id)): Path<(String, String)>) -> NoContent {
	shared
		.store
		.lock()
		.connections
		.retain(|connection| connection["type"] != kind || connection["id"] != id);
	shared.hub.broadcast("USER_CONNECTIONS_UPDATE", &json!({}));
	Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_mention(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.store.lock().dismissed_mentions.insert(id);
	shared
		.hub
		.broadcast("RECENT_MENTION_DELETE", &json!({ "message_id": id.to_string() }));
	Ok(StatusCode::NO_CONTENT)
}

pub async fn put_note(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> NoContent {
	shared.set_note(id, body["note"].as_str().unwrap_or_default())?;
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

pub async fn patch_settings_proto(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	let encoded = body["settings"]
		.as_str()
		.ok_or_else(|| ApiError::invalid_body("settings is required"))?;
	let patch = decode_settings(encoded).ok_or_else(|| ApiError::invalid_body("settings is not valid"))?;
	let settings = {
		let mut store = shared.store.lock();
		if let Some(status) = patch.status_bytes {
			store.proto_status = status;
		}
		if let Some(privacy) = patch.privacy_bytes {
			store.proto_privacy = privacy;
		}
		if let Some(text_and_images) = patch.text_and_images_bytes {
			store.proto_text_and_images = text_and_images;
		}
		if let Some(guild_folders) = patch.guild_folders {
			store.proto_guild_folders = Some(guild_folders);
		}
		user_settings_proto(&shared.fixtures, &store)
	};
	shared.hub.broadcast(
		"USER_SETTINGS_PROTO_UPDATE",
		&json!({ "settings": { "type": 1, "proto": encoded }, "partial": true }),
	);
	Ok(Json(json!({ "settings": settings })))
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

pub async fn delete_guild(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.leave_guild(id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn guild_pincode(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.send_ownership_code(id)?;
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
	if let Some(application) = shared.application_invite(&code, true) {
		return Ok(Json(application));
	}
	shared.accept_invite(&code).map(Json)
}

pub async fn member_verification(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.member_verification_json(id).map(Json)
}

pub async fn submit_join_request(
	State(shared): Mock,
	Path(id): Path<u64>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared.submit_join_request(id, &body).map(Json)
}

pub async fn delete_join_request(State(shared): Mock, Path(id): Path<u64>) -> NoContent {
	shared.delete_join_request(id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn join_request_guilds(State(shared): Mock) -> Json<Value> {
	Json(shared.join_request_guilds_json())
}

pub async fn reject_application(State(shared): Mock) -> StatusCode {
	shared.reject_application();
	StatusCode::NO_CONTENT
}

pub async fn create_guild(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	shared.create_guild(&body).map(Json)
}

pub async fn auth_sessions(State(shared): Mock) -> Json<Value> {
	Json(shared.auth_sessions_json())
}

pub async fn logout_sessions(State(shared): Mock, headers: HeaderMap, Json(body): Json<Value>) -> NoContent {
	if !headers.contains_key(MFA_AUTHORIZATION) {
		return Err(ApiError::mfa_required());
	}
	shared.end_sessions(&body["session_id_hashes"]);
	Ok(StatusCode::NO_CONTENT)
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

#[derive(serde::Deserialize)]
pub struct BanParams {
	after: Option<u64>,
	limit: Option<usize>,
}

pub async fn bans(State(shared): Mock, Path(id): Path<u64>, Query(params): Query<BanParams>) -> ApiResult {
	shared.bans_json(id, params.after, params.limit).map(Json)
}

pub async fn automod_rules(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.automod_rules_json(id).map(Json)
}

pub async fn create_automod_rule(
	State(shared): Mock,
	Path(id): Path<u64>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared.create_automod_rule(id, body).map(Json)
}

pub async fn modify_automod_rule(
	State(shared): Mock,
	Path((id, rule_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared.modify_automod_rule(id, rule_id, &body).map(Json)
}

pub async fn delete_automod_rule(State(shared): Mock, Path((id, rule_id)): Path<(u64, u64)>) -> NoContent {
	shared.delete_automod_rule(id, rule_id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn active_threads(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	let guild_id = protocol::Snowflake(id);
	Ok(Json(shared.threads_json(|store, thread| {
		thread.guild_id == Some(guild_id)
			&& store.thread_visible(thread)
			&& thread
				.thread_metadata
				.as_ref()
				.is_none_or(|metadata| !metadata.archived)
	})))
}

#[derive(serde::Deserialize)]
pub struct ArchivedParams {
	before: Option<String>,
	limit: Option<usize>,
}

fn page_threads<K: Ord>(
	threads: &mut Vec<&Channel>,
	key: impl Fn(&Channel) -> K,
	before: Option<K>,
	limit: usize,
) -> bool {
	threads.sort_by_key(|thread| std::cmp::Reverse(key(thread)));
	if let Some(before) = before {
		threads.retain(|thread| key(thread) < before);
	}
	let has_more = threads.len() > limit;
	threads.truncate(limit);
	has_more
}

fn archived_in(
	shared: &Shared,
	parent: u64,
	params: ArchivedParams,
	by_timestamp: bool,
	scope: impl Fn(&Store, &Channel) -> bool,
) -> ApiResult {
	let parent_id = protocol::Snowflake(parent);
	let limit = params.limit.unwrap_or(DEFAULT_ARCHIVED_LIMIT);
	let archived_at = |thread: &Channel| {
		thread
			.thread_metadata
			.as_ref()
			.and_then(|metadata| metadata.archive_timestamp)
			.unwrap_or_default()
	};
	Ok(Json(shared.threads_page(
		|store, thread| {
			thread.parent_id == Some(parent_id)
				&& scope(store, thread)
				&& thread
					.thread_metadata
					.as_ref()
					.is_some_and(|metadata| metadata.archived)
		},
		|threads| {
			if by_timestamp {
				let before = params
					.before
					.as_deref()
					.and_then(|before| chrono::DateTime::parse_from_rfc3339(before).ok())
					.map(|before| before.with_timezone(&chrono::Utc));
				page_threads(threads, archived_at, before, limit)
			} else {
				let before = params.before.as_deref().and_then(|before| before.parse().ok());
				page_threads(threads, |thread| thread.id.0, before, limit)
			}
		},
	)))
}

pub async fn archived_threads(
	State(shared): Mock,
	Path(id): Path<u64>,
	Query(params): Query<ArchivedParams>,
) -> ApiResult {
	archived_in(&shared, id, params, true, |_, thread| {
		thread.kind != ChannelType::PrivateThread
	})
}

pub async fn archived_private_threads(
	State(shared): Mock,
	Path(id): Path<u64>,
	Query(params): Query<ArchivedParams>,
) -> ApiResult {
	archived_in(&shared, id, params, true, |_, thread| {
		thread.kind == ChannelType::PrivateThread
	})
}

pub async fn joined_archived_private_threads(
	State(shared): Mock,
	Path(id): Path<u64>,
	Query(params): Query<ArchivedParams>,
) -> ApiResult {
	archived_in(&shared, id, params, false, |store, thread| {
		thread.kind == ChannelType::PrivateThread && store.joined_threads.contains(&thread.id.0)
	})
}

pub async fn modify_guild(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.modify_guild(id, &body).map(Json)
}

pub async fn create_channel(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.create_channel(id, &body).map(Json)
}

pub async fn reorder_channels(
	State(shared): Mock,
	Path(id): Path<u64>,
	Json(body): Json<Value>,
) -> NoContent {
	shared.reorder_channels(id, &body)?;
	Ok(StatusCode::NO_CONTENT)
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

pub async fn role_member_counts(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.role_member_counts(id).map(Json)
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

pub async fn set_voice_status(
	State(shared): Mock,
	Path(id): Path<u64>,
	Json(body): Json<Value>,
) -> NoContent {
	shared.set_voice_status(id, &body)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_regions() -> Json<Value> {
	Json(json!([
		{ "id": "us-east", "name": "US East", "optimal": true, "deprecated": false, "custom": false },
		{ "id": "us-west", "name": "US West", "optimal": false, "deprecated": false, "custom": false },
		{ "id": "rotterdam", "name": "Rotterdam", "optimal": false, "deprecated": false, "custom": false },
		{ "id": "japan", "name": "Japan", "optimal": false, "deprecated": false, "custom": false },
		{ "id": "sydney", "name": "Sydney", "optimal": false, "deprecated": false, "custom": false },
		{ "id": "vip-us-east", "name": "VIP US East", "optimal": false, "deprecated": true, "custom": false },
	]))
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

pub async fn add_thread_member(State(shared): Mock, Path((id, user_id)): Path<(u64, u64)>) -> NoContent {
	shared.thread_member_added(id, user_id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn thread_settings(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.update_thread_settings(id, &body).map(Json)
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

pub async fn accept_message_request(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.accept_message_request(id).map(Json)
}

pub async fn reject_message_request(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	let mut rejected = shared.reject_message_requests(&[id])?;
	Ok(Json(rejected.pop().unwrap_or(Value::Null)))
}

pub async fn reject_message_requests(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	let ids: Vec<u64> = body["channel_ids"]
		.as_array()
		.map(|ids| ids.iter().filter_map(parse_id).collect())
		.unwrap_or_default();
	shared.reject_message_requests(&ids).map(Value::Array).map(Json)
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
	let burst = super::channels::is_burst(params.get("type"));
	shared
		.reaction_users(id, message_id, &emoji, limit, burst)
		.map(Json)
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
	let guild_badges = if guild_id.is_some() && user_id.is_multiple_of(2) {
		json!([{ "id": "guild_booster_lvl1", "description": "Booster", "icon": "51040c70d4f20a921ad6674ff86fc95c" }])
	} else {
		json!([])
	};
	let profile_effect = user_id
		.is_multiple_of(2)
		.then(|| json!({ "id": "1139323098643333240", "expires_at": null }));
	let theme_colors = user
		.and_then(|user| user.display_name_styles.as_ref())
		.and_then(|style| Some([*style.colors.first()?, *style.colors.last()?]));
	let tagged = user.is_some_and(|user| user.primary_guild.is_some());
	let decorated = user.is_some_and(|user| user.avatar_decoration_data.is_some());
	json!({
		"mutual_friends": friends,
		"badges": badges,
		"guild_badges": guild_badges,
		"profile_effect": profile_effect,
		"theme_colors": theme_colors,
		"legacy_username": user.filter(|_| tagged).map(|user| format!("{}#1234", user.username)),
		"private": tagged && decorated,
		"connected_accounts": if user_id == crate::fixtures::CURRENT_USER_ID {
			store
				.connections
				.iter()
				.filter(|connection| connection["visibility"] == 1)
				.cloned()
				.collect()
		} else {
			user.map_or_else(Vec::new, |user| {
				vec![json!({ "type": "github", "id": user.id.to_string(), "name": user.username, "verified": true })]
			})
		},
		"guild_member_profile": guild_id.and_then(|guild_id| {
			let guild = fixtures.guild(guild_id)?;
			let bio = store.guild_bios.get(&(guild_id, user_id)).cloned().or_else(|| {
				(user_id == crate::fixtures::CURRENT_USER_ID)
					.then(|| format!("Regular in {}", guild.properties.name.clone().unwrap_or_default()))
			})?;
			Some(json!({ "guild_id": guild_id.to_string(), "bio": bio }))
		}),
		"guild_member": guild_id.and_then(|guild_id| {
			let guild = fixtures.guild(guild_id)?;
			store.member_now(guild, user_id).map(|member| to_json(&member))
		}),
	})
}

pub async fn voice_elsewhere(State(shared): Mock) -> NoContent {
	shared.voice_elsewhere()?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_move(State(shared): Mock) -> NoContent {
	shared.voice_move()?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_server_mute(State(shared): Mock, Json(body): Json<Value>) -> NoContent {
	shared.set_server_voice(
		body["mute"].as_bool().unwrap_or(false),
		body["deaf"].as_bool().unwrap_or(false),
	)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_reassign(State(shared): Mock) -> NoContent {
	shared.voice_reassign()?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_dave_failure(State(shared): Mock) -> NoContent {
	shared.voice_dave_failure()?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_guest_join(State(shared): Mock) -> NoContent {
	shared.guest_voice(true)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_guest_leave(State(shared): Mock) -> NoContent {
	shared.guest_voice(false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn call_incoming(State(shared): Mock) -> NoContent {
	shared.incoming_call()?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn voice_close(State(shared): Mock, Path(code): Path<u16>) -> NoContent {
	shared.voice_close(code);
	Ok(StatusCode::NO_CONTENT)
}
