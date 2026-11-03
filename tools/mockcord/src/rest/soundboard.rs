use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::error::{ApiError, ApiResult};
use crate::json::{parse_id, to_json};
use crate::media;
use crate::soundboard::default_sounds;
use crate::state::Shared;

type Mock = State<Arc<Shared>>;
type NoContent = Result<StatusCode, ApiError>;

pub fn routes() -> Router<Arc<Shared>> {
	Router::new()
		.route("/soundboard-default-sounds", get(list_default))
		.route(
			"/guilds/{id}/soundboard-sounds",
			get(list_guild).post(create_guild),
		)
		.route(
			"/guilds/{id}/soundboard-sounds/{sid}",
			get(get_guild).patch(edit_guild).delete(delete_guild),
		)
		.route("/channels/{id}/send-soundboard-sound", post(send))
		.route("/channels/{id}/voice-channel-effects", post(send_effect))
}

pub async fn file(State(shared): Mock, Path(id): Path<u64>, headers: HeaderMap) -> Response {
	match shared.sound_file(id).and_then(media::asset) {
		Some((kind, bytes)) => media::serve_asset(kind, bytes, &headers),
		None => ApiError::not_found().into_response(),
	}
}

async fn list_default() -> ApiResult {
	Ok(Json(to_json(&default_sounds())))
}

async fn list_guild(State(shared): Mock, Path(id): Path<u64>) -> ApiResult {
	Ok(Json(json!({ "items": to_json(&shared.guild_sounds(id)?) })))
}

async fn get_guild(State(shared): Mock, Path((id, sound_id)): Path<(u64, u64)>) -> ApiResult {
	shared
		.guild_sounds(id)?
		.iter()
		.find(|sound| sound.sound_id.0 == sound_id)
		.map(|sound| Json(to_json(sound)))
		.ok_or_else(ApiError::not_found)
}

async fn create_guild(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> ApiResult {
	Ok(Json(to_json(&shared.create_sound(id, &body)?)))
}

async fn edit_guild(
	State(shared): Mock,
	Path((id, sound_id)): Path<(u64, u64)>,
	Json(body): Json<Value>,
) -> ApiResult {
	Ok(Json(to_json(&shared.edit_sound(id, sound_id, &body)?)))
}

async fn delete_guild(State(shared): Mock, Path((id, sound_id)): Path<(u64, u64)>) -> NoContent {
	shared.delete_sound(id, sound_id)?;
	Ok(StatusCode::NO_CONTENT)
}

async fn send(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> NoContent {
	let sound_id =
		parse_id(&body["sound_id"]).ok_or_else(|| ApiError::invalid_body("sound_id is required"))?;
	shared.send_sound(id, sound_id)?;
	Ok(StatusCode::NO_CONTENT)
}

async fn send_effect(State(shared): Mock, Path(id): Path<u64>, Json(body): Json<Value>) -> NoContent {
	shared.send_effect(id, &body)?;
	Ok(StatusCode::NO_CONTENT)
}
