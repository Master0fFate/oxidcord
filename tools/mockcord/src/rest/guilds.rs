use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::Value;

use super::channels::search_in;
use crate::error::{ApiError, ApiResult};
use crate::fixtures::GuildFixture;
use crate::json::to_json;
use crate::ready::guild_threads;
use crate::state::Shared;

const DEFAULT_MEMBER_SEARCH_LIMIT: usize = 1;
const MAX_MEMBER_SEARCH_LIMIT: usize = 1000;

fn find_guild(shared: &Shared, id: u64) -> Result<&GuildFixture, ApiError> {
	shared.fixtures.guild(id).ok_or_else(ApiError::unknown_guild)
}

pub async fn guild(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
	let guild = find_guild(&shared, id)?;
	let store = shared.store.lock();
	let mut value = to_json(&store.properties[&guild.id]);
	value["roles"] = to_json(&store.roles[&guild.id]);
	value["emojis"] = to_json(&store.emojis[&guild.id]);
	value["stickers"] = Value::Array(Vec::new());
	if params.get("with_counts").is_some_and(|flag| flag == "true") {
		value["approximate_member_count"] = store.live_member_count(guild).into();
		value["approximate_presence_count"] = store.online_count(guild.id).into();
	}
	Ok(Json(value))
}

pub async fn channels(State(shared): State<Arc<Shared>>, Path(id): Path<u64>) -> ApiResult {
	let guild = find_guild(&shared, id)?;
	let store = shared.store.lock();
	Ok(Json(to_json(&store.guild_channels(guild))))
}

pub async fn roles(State(shared): State<Arc<Shared>>, Path(id): Path<u64>) -> ApiResult {
	find_guild(&shared, id)?;
	Ok(Json(to_json(&shared.store.lock().roles[&id])))
}

pub async fn emojis(State(shared): State<Arc<Shared>>, Path(id): Path<u64>) -> ApiResult {
	find_guild(&shared, id)?;
	Ok(Json(to_json(&shared.store.lock().emojis[&id])))
}

#[derive(Deserialize)]
pub struct MemberSearch {
	query: Option<String>,
	limit: Option<usize>,
}

pub async fn search_members(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	Query(params): Query<MemberSearch>,
) -> ApiResult {
	let guild = find_guild(&shared, id)?;
	let store = shared.store.lock();
	let query = params.query.unwrap_or_default().to_lowercase();
	let limit = params
		.limit
		.unwrap_or(DEFAULT_MEMBER_SEARCH_LIMIT)
		.clamp(1, MAX_MEMBER_SEARCH_LIMIT);
	let matches: Vec<_> = guild
		.members
		.iter()
		.filter(|member| {
			member
				.user
				.as_ref()
				.is_none_or(|user| !store.is_removed(id, user.id.0))
		})
		.filter(|member| {
			member
				.nick
				.as_deref()
				.is_some_and(|nick| nick.to_lowercase().starts_with(&query))
				|| member.user.as_ref().is_some_and(|user| {
					user.username.to_lowercase().starts_with(&query)
						|| user
							.global_name
							.as_deref()
							.is_some_and(|name| name.to_lowercase().starts_with(&query))
				})
		})
		.take(limit)
		.collect();
	Ok(Json(to_json(&matches)))
}

pub async fn member(State(shared): State<Arc<Shared>>, Path((id, user_id)): Path<(u64, u64)>) -> ApiResult {
	let guild = find_guild(&shared, id)?;
	shared
		.store
		.lock()
		.member_now(guild, user_id)
		.map(|member| Json(to_json(&member)))
		.ok_or_else(ApiError::unknown_member)
}

pub async fn search_messages(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
	let guild = find_guild(&shared, id)?;
	let store = shared.store.lock();
	let mut channel_ids: Vec<u64> = guild.message_channel_ids.clone();
	channel_ids.extend(
		guild_threads(&store, guild.id)
			.into_iter()
			.map(|thread| thread.id.0),
	);
	Ok(Json(search_in(&store, &channel_ids, &params)))
}
