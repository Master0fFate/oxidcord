use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::fixtures::CURRENT_USER_ID;
use crate::state::Shared;

type Mock = State<Arc<Shared>>;
type NoContent = Result<StatusCode, ApiError>;

const CURRENT_USER_SEGMENT: &str = "@me";

pub async fn create(State(shared): Mock, Json(body): Json<Value>) -> ApiResult {
	shared.create_stage_instance(&body).map(Json)
}

pub async fn get(State(shared): Mock, Path(channel_id): Path<u64>) -> ApiResult {
	shared.stage_instance(channel_id).map(Json)
}

pub async fn modify(State(shared): Mock, Path(channel_id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	shared.modify_stage_instance(channel_id, &body).map(Json)
}

pub async fn delete(State(shared): Mock, Path(channel_id): Path<u64>) -> NoContent {
	shared.delete_stage_instance(channel_id)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn patch_voice_state(
	State(shared): Mock,
	Path((guild_id, target)): Path<(u64, String)>,
	Json(body): Json<Value>,
) -> NoContent {
	let user_id = if target == CURRENT_USER_SEGMENT {
		CURRENT_USER_ID
	} else {
		target.parse().map_err(|_| ApiError::unknown_user())?
	};
	shared.patch_voice_state(guild_id, user_id, &body)?;
	Ok(StatusCode::NO_CONTENT)
}
