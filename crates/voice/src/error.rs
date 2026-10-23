use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
	#[error("voice endpoint is not valid")]
	InvalidEndpoint,
	#[error("voice endpoint must use wss, or ws on a loopback host")]
	InsecureEndpoint,
	#[error("voice server closed the connection with code {0}")]
	Closed(u16),
	#[error("voice handshake timed out")]
	Timeout,
	#[error("voice handshake failed: {0}")]
	Handshake(&'static str),
	#[error("voice server offered no supported encryption mode")]
	NoEncryptionMode,
	#[error("voice encryption key is invalid")]
	InvalidKey,
	#[error("voice ip discovery failed")]
	IpDiscovery,
	#[error("voice websocket error: {0}")]
	Transport(String),
	#[error("voice udp error: {0}")]
	Udp(#[from] std::io::Error),
	#[error("opus error: {0}")]
	Opus(#[from] opus::Error),
	#[error("resampler error: {0}")]
	Resample(String),
	#[error("audio error: {0}")]
	Audio(String),
	#[error("dave: {0}")]
	Dave(String),
	#[error("video: {0}")]
	Video(#[from] crate::video::VideoError),
	#[error("failed to encode a voice gateway frame: {0}")]
	Encode(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, VoiceError>;
