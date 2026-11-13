use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, LoginError>;

#[derive(Debug, Error)]
pub enum LoginError {
	#[error("the token was rejected")]
	InvalidToken,
	#[error("network error: {0}")]
	Network(String),
	#[error("unexpected response: {0}")]
	Protocol(String),
	#[error("login cancelled")]
	Cancelled,
	#[error("login session expired")]
	Expired,
	#[error("Discord wants a captcha for this login, use the web login")]
	Captcha,
	#[error("login helper failed: {0}")]
	Helper(String),
	#[error("storage error: {0}")]
	Storage(String),
	#[error("io error: {0}")]
	Io(#[from] io::Error),
}

impl From<rest::RestError> for LoginError {
	fn from(error: rest::RestError) -> Self {
		match error {
			rest::RestError::Unauthorized(_) => Self::InvalidToken,
			rest::RestError::CaptchaRequired(_) => Self::Captcha,
			rest::RestError::Network(reason) => Self::Network(reason),
			other => match other.status_code() {
				Some(status) => {
					let api = other
						.api()
						.map(|api| format!(", API {}", api.code))
						.unwrap_or_default();
					Self::Protocol(format!("Discord refused the request (HTTP {status}{api})"))
				}
				None => Self::Protocol(other.to_string()),
			},
		}
	}
}

impl From<store::StoreError> for LoginError {
	fn from(error: store::StoreError) -> Self {
		match error {
			store::StoreError::Io(error) => Self::Io(error),
			other => Self::Storage(other.to_string()),
		}
	}
}
