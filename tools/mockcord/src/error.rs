use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ApiError {
	status: StatusCode,
	code: i64,
	message: &'static str,
}

pub type ApiResult<T = Json<Value>> = Result<T, ApiError>;

impl ApiError {
	const fn new(status: StatusCode, code: i64, message: &'static str) -> Self {
		Self {
			status,
			code,
			message,
		}
	}

	pub const fn unauthorized() -> Self {
		Self::new(StatusCode::UNAUTHORIZED, 0, "401: Unauthorized")
	}

	pub const fn not_found() -> Self {
		Self::new(StatusCode::NOT_FOUND, 0, "404: Not Found")
	}

	pub const fn unknown_channel() -> Self {
		Self::new(StatusCode::NOT_FOUND, 10003, "Unknown Channel")
	}

	pub const fn unknown_guild() -> Self {
		Self::new(StatusCode::NOT_FOUND, 10004, "Unknown Guild")
	}

	pub const fn unknown_invite() -> Self {
		Self::new(StatusCode::NOT_FOUND, 10006, "Unknown Invite")
	}

	pub const fn unknown_member() -> Self {
		Self::new(StatusCode::NOT_FOUND, 10007, "Unknown Member")
	}

	pub const fn unknown_message() -> Self {
		Self::new(StatusCode::NOT_FOUND, 10008, "Unknown Message")
	}

	pub const fn unknown_user() -> Self {
		Self::new(StatusCode::NOT_FOUND, 10013, "Unknown User")
	}

	pub const fn invalid_body(message: &'static str) -> Self {
		Self::new(StatusCode::BAD_REQUEST, 50035, message)
	}

	pub const fn empty_message() -> Self {
		Self::new(StatusCode::BAD_REQUEST, 50006, "Cannot send an empty message")
	}

	pub const fn not_author() -> Self {
		Self::new(
			StatusCode::FORBIDDEN,
			50005,
			"Cannot edit a message authored by another user",
		)
	}

	pub const fn wrong_channel_kind() -> Self {
		Self::new(
			StatusCode::BAD_REQUEST,
			50024,
			"Cannot execute action on this channel type",
		)
	}
}

impl IntoResponse for ApiError {
	fn into_response(self) -> Response {
		(
			self.status,
			Json(json!({ "message": self.message, "code": self.code })),
		)
			.into_response()
	}
}
