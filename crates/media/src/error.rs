use std::sync::Arc;

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum MediaError {
	#[error("fetch failed: {0}")]
	Fetch(String),
	#[error("decode failed: {0}")]
	Decode(String),
	#[error("limit exceeded: {0}")]
	Limit(String),
	#[error("io error: {0}")]
	Io(Arc<std::io::Error>),
	#[error("host not allowed: {0}")]
	DisallowedHost(String),
}

pub type Result<T> = std::result::Result<T, MediaError>;

impl From<std::io::Error> for MediaError {
	fn from(error: std::io::Error) -> Self {
		Self::Io(Arc::new(error))
	}
}

impl From<image::ImageError> for MediaError {
	fn from(error: image::ImageError) -> Self {
		match error {
			image::ImageError::Limits(error) => Self::Limit(error.to_string()),
			other => Self::Decode(other.to_string()),
		}
	}
}

pub fn redact_url(url: &str) -> &str {
	url.split(['?', '#']).next().unwrap_or(url)
}
