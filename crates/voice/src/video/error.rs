use thiserror::Error;

#[derive(Debug, Error)]
pub enum VideoError {
	#[error("video is not supported on this platform")]
	Unsupported,
	#[error("video device or capture target not found")]
	NotFound,
	#[error("a video stream is already active on this connection")]
	Busy,
	#[error("video capture timed out")]
	Timeout,
	#[error("video frame is invalid: {0}")]
	InvalidFrame(&'static str),
	#[error("video device error: {0}")]
	Device(String),
	#[error("h264 encode error: {0}")]
	Encode(String),
	#[error("h264 decode error: {0}")]
	Decode(String),
}
