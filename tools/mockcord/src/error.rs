use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ApiError {
	status: StatusCode,
	code: i64,
	message: &'static str,
	mfa: bool,
	retry_after: Option<f64>,
}

pub type ApiResult<T = Json<Value>> = Result<T, ApiError>;

impl ApiError {
	const fn new(status: StatusCode, code: i64, message: &'static str) -> Self {
		Self {
			status,
			code,
			message,
			mfa: false,
			retry_after: None,
		}
	}

	pub const fn server_error() -> Self {
		Self::new(StatusCode::INTERNAL_SERVER_ERROR, 0, "500: Internal Server Error")
	}

	pub const fn slowmode(retry_after: f64) -> Self {
		Self {
			retry_after: Some(retry_after),
			..Self::new(
				StatusCode::TOO_MANY_REQUESTS,
				20016,
				"This action cannot be performed due to slowmode rate limit.",
			)
		}
	}

	pub const fn automod_blocked() -> Self {
		Self::new(StatusCode::BAD_REQUEST, 200000, "Message blocked by AutoMod")
	}

	pub const fn file_too_large() -> Self {
		Self::new(StatusCode::PAYLOAD_TOO_LARGE, 40005, "Request entity too large")
	}

	pub const fn unauthorized() -> Self {
		Self::new(StatusCode::UNAUTHORIZED, 0, "401: Unauthorized")
	}

	pub const fn mfa_required() -> Self {
		Self {
			mfa: true,
			..Self::new(
				StatusCode::UNAUTHORIZED,
				60003,
				"Two factor is required for this operation",
			)
		}
	}

	pub const fn cannot_message_user() -> Self {
		Self::new(StatusCode::FORBIDDEN, 50007, "Cannot send messages to this user")
	}

	pub const fn missing_permissions() -> Self {
		Self::new(StatusCode::FORBIDDEN, 50013, "Missing Permissions")
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

	pub const fn unknown_tag() -> Self {
		Self::new(StatusCode::BAD_REQUEST, 80004, "No users with DiscordTag exist")
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
		let mut body = json!({ "message": self.message, "code": self.code });
		if let Some(retry_after) = self.retry_after {
			body["retry_after"] = json!(retry_after);
			body["global"] = json!(false);
		}
		if self.mfa {
			body["mfa"] = json!({ "ticket": "mock-mfa-ticket", "methods": [] });
		}
		if let Some(retry_after) = self.retry_after {
			body["retry_after"] = json!(retry_after);
			body["global"] = json!(false);
		}
		(self.status, Json(body)).into_response()
	}
}
