use std::time::Duration;

use protocol::DecodeError;
use protocol::rest::ApiError;
use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, RestError>;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CaptchaChallenge {
	#[serde(default)]
	pub sitekey: Option<String>,
	#[serde(default)]
	pub service: Option<String>,
	#[serde(default)]
	pub session_id: Option<String>,
	#[serde(default)]
	pub rqdata: Option<String>,
	#[serde(default)]
	pub rqtoken: Option<String>,
	#[serde(default)]
	pub invisible: bool,
}

#[derive(Debug, Error)]
pub enum RestError {
	#[error("http status {0}")]
	Http(StatusCode, Option<ApiError>),
	#[error("rate limited, retry in {retry_after:?}")]
	RateLimited { retry_after: Duration },
	#[error("slowmode is active")]
	Slowmode { retry_after: Option<Duration> },
	#[error("captcha required")]
	CaptchaRequired(Box<CaptchaChallenge>),
	#[error("network error: {0}")]
	Network(String),
	#[error(transparent)]
	Decode(DecodeError),
	#[error("unexpected redirect")]
	Redirect,
	#[error("unauthorized")]
	Unauthorized(Option<ApiError>),
	#[error("two-factor confirmation required")]
	MfaRequired,
	#[error("forbidden")]
	Forbidden(Option<ApiError>),
	#[error("not found")]
	NotFound(Option<ApiError>),
	#[error("search index is not ready, retry in {retry_after:?}")]
	IndexNotReady { retry_after: Duration },
	#[error("response body too large")]
	TooLarge,
	#[error("invalid request")]
	InvalidRequest,
	#[error("invalid settings payload")]
	InvalidSettings,
	#[error("settings changed while updating")]
	OutOfDate,
}

const INVALID_TOKEN_CODE: i64 = 40001;

impl RestError {
	pub fn api(&self) -> Option<&ApiError> {
		match self {
			Self::Http(_, api) | Self::Unauthorized(api) | Self::Forbidden(api) | Self::NotFound(api) => {
				api.as_ref()
			}
			_ => None,
		}
	}

	pub fn is_dead_session(&self) -> bool {
		matches!(self, Self::Unauthorized(api)
			if api.as_ref().is_none_or(|api| api.code == 0 || api.code == INVALID_TOKEN_CODE))
	}
}

pub(crate) fn network_error(error: reqwest::Error) -> RestError {
	let reason = if error.is_timeout() {
		"request timed out"
	} else if error.is_connect() {
		"connection failed"
	} else if error.is_body() || error.is_decode() {
		"body read failed"
	} else {
		"request failed"
	};
	RestError::Network(reason.to_owned())
}
