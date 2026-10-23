#[cfg(windows)]
mod media_foundation;
mod software;

use std::time::Duration;

use super::{VideoError, VideoFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderBackend {
	MediaFoundation,
	OpenH264,
}

enum Backend {
	#[cfg(windows)]
	MediaFoundation(media_foundation::MediaFoundationDecoder),
	OpenH264(software::SoftwareDecoder),
}

pub struct H264Decoder {
	backend: Backend,
}

impl H264Decoder {
	pub fn new() -> Result<Self, VideoError> {
		Ok(Self {
			backend: Self::open()?,
		})
	}

	#[cfg(windows)]
	fn open() -> Result<Backend, VideoError> {
		match media_foundation::MediaFoundationDecoder::new() {
			Ok(decoder) => Ok(Backend::MediaFoundation(decoder)),
			Err(error) => {
				tracing::debug!(%error, "media foundation h264 decoder unavailable");
				Self::open_software()
			}
		}
	}

	#[cfg(not(windows))]
	fn open() -> Result<Backend, VideoError> {
		Self::open_software()
	}

	fn open_software() -> Result<Backend, VideoError> {
		Ok(Backend::OpenH264(software::SoftwareDecoder::new()?))
	}

	pub fn backend(&self) -> DecoderBackend {
		match self.backend {
			#[cfg(windows)]
			Backend::MediaFoundation(_) => DecoderBackend::MediaFoundation,
			Backend::OpenH264(_) => DecoderBackend::OpenH264,
		}
	}

	pub fn decode(&mut self, frame: &[u8], timestamp: Duration) -> Result<Option<VideoFrame>, VideoError> {
		match &mut self.backend {
			#[cfg(windows)]
			Backend::MediaFoundation(decoder) => decoder.decode(frame, timestamp),
			Backend::OpenH264(decoder) => decoder.decode(frame, timestamp),
		}
	}
}
