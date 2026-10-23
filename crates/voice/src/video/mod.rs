pub mod capture;
mod convert;
pub mod decode;
pub mod encode;
mod error;
pub(crate) mod hub;
#[cfg(windows)]
mod mf;
pub mod nal;
pub(crate) mod receive;
pub mod rtp;
pub(crate) mod send;

use std::sync::Arc;
use std::time::Duration;

use protocol::Snowflake;

pub use error::VideoError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
	Nv12,
	Bgra,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
	pub width: u32,
	pub height: u32,
	pub format: PixelFormat,
	pub data: Vec<u8>,
	pub timestamp: Duration,
}

impl VideoFrame {
	pub fn expected_len(&self) -> usize {
		let pixels = self.width as usize * self.height as usize;
		match self.format {
			PixelFormat::Nv12 => pixels + pixels / 2,
			PixelFormat::Bgra => pixels * 4,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoQuality {
	pub width: u32,
	pub height: u32,
	pub fps: u32,
	pub bitrate: u32,
}

impl Default for VideoQuality {
	fn default() -> Self {
		Self {
			width: 1280,
			height: 720,
			fps: 30,
			bitrate: 2_000_000,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoKind {
	Camera,
	Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoStream {
	pub user_id: Snowflake,
	pub ssrc: u32,
	pub kind: VideoKind,
}

pub trait VideoSink: Send + Sync {
	fn frame(&self, stream: VideoStream, frame: Arc<VideoFrame>);

	fn ended(&self, _stream: VideoStream) {}
}
