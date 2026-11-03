use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::state::Shared;

type Mock = State<Arc<Shared>>;
type NoContent = Result<StatusCode, ApiError>;

pub async fn preview(State(shared): Mock, Path(key): Path<String>) -> ApiResult {
	shared.stream_preview(&key).map(Json)
}

pub async fn upload_preview(
	State(shared): Mock,
	Path(key): Path<String>,
	Json(body): Json<Value>,
) -> NoContent {
	shared.store_stream_preview(&key, &body)?;
	Ok(StatusCode::NO_CONTENT)
}

pub async fn start(State(shared): Mock) -> NoContent {
	shared.start_stream()?;
	Ok(StatusCode::NO_CONTENT)
}
