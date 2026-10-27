use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum PlayerError {
	#[error("source: {0}")]
	Source(String),
	#[error("format: {0}")]
	Format(String),
	#[error("unsupported codec: {0}")]
	Unsupported(String),
	#[error("audio: {0}")]
	Audio(String),
	#[error("video: {0}")]
	Video(String),
}

pub type Result<T> = std::result::Result<T, PlayerError>;

impl From<symphonia::core::errors::Error> for PlayerError {
	fn from(error: symphonia::core::errors::Error) -> Self {
		Self::Format(error.to_string())
	}
}

impl From<voice::VoiceError> for PlayerError {
	fn from(error: voice::VoiceError) -> Self {
		Self::Audio(error.to_string())
	}
}
