use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::state::Shared;

type Mock = State<Arc<Shared>>;

pub async fn list(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	shared.scheduled_events_json(id).map(Json)
}

pub async fn create(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.create_scheduled_event(id, &body).map(Json)
}

pub async fn modify(
	State(shared): Mock,
	Path((id, event_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	shared.modify_scheduled_event(id, event_id, &body).map(Json)
}

pub async fn delete(
	State(shared): Mock,
	Path((id, event_id)): Path<(u64, u64)>,
) -> Result<StatusCode, ApiError> {
	shared.delete_scheduled_event(id, event_id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn join(State(shared): Mock, Path((id, event_id)): Path<(u64, u64)>) -> ApiResult {
	shared.set_event_interest(id, event_id, true).map(Json)
}

pub async fn leave(
	State(shared): Mock,
	Path((id, event_id)): Path<(u64, u64)>,
) -> Result<StatusCode, ApiError> {
	shared.set_event_interest(id, event_id, false)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn rsvps(State(shared): Mock, Query(params): Query<HashMap<String, String>>) -> Json<Value> {
	let guild_ids: Vec<u64> = params
		.get("guild_ids")
		.map(|ids| ids.split(',').filter_map(|id| id.parse().ok()).collect())
		.unwrap_or_default();
	Json(shared.event_rsvps_json(&guild_ids))
}
