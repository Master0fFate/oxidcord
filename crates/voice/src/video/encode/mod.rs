#[cfg(windows)]
mod media_foundation;
mod software;

use super::{VideoError, VideoFrame, VideoKind, convert};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
	pub width: u32,
	pub height: u32,
	pub fps: u32,
	pub bitrate: u32,
	pub keyframe_seconds: u32,
	pub kind: VideoKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
	pub data: Vec<u8>,
	pub keyframe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
	MediaFoundationHardware,
	MediaFoundationSoftware,
	OpenH264,
}

enum Backend {
	#[cfg(windows)]
	MediaFoundation(media_foundation::MediaFoundationEncoder),
	OpenH264(Box<software::SoftwareEncoder>),
}

pub struct H264Encoder {
	backend: Backend,
	kind: EncoderBackend,
	config: EncoderConfig,
	keyframe_requested: bool,
	scratch: Vec<u8>,
}

impl H264Encoder {
	pub fn new(config: EncoderConfig) -> Result<Self, VideoError> {
		if config.width < 2
			|| config.height < 2
			|| !config.width.is_multiple_of(2)
			|| !config.height.is_multiple_of(2)
		{
			return Err(VideoError::InvalidFrame("encoder dimensions must be even"));
		}
		let (backend, kind) = Self::open(&config)?;
		Ok(Self {
			backend,
			kind,
			config,
			keyframe_requested: true,
			scratch: Vec::new(),
		})
	}

	#[cfg(windows)]
	fn open(config: &EncoderConfig) -> Result<(Backend, EncoderBackend), VideoError> {
		for hardware in [true, false] {
			match media_foundation::MediaFoundationEncoder::new(*config, hardware) {
				Ok(encoder) => {
					let kind = if hardware {
						EncoderBackend::MediaFoundationHardware
					} else {
						EncoderBackend::MediaFoundationSoftware
					};
					return Ok((Backend::MediaFoundation(encoder), kind));
				}
				Err(error) => tracing::debug!(hardware, %error, "media foundation h264 encoder unavailable"),
			}
		}
		Self::open_software(config)
	}

	#[cfg(not(windows))]
	fn open(config: &EncoderConfig) -> Result<(Backend, EncoderBackend), VideoError> {
		Self::open_software(config)
	}

	fn open_software(config: &EncoderConfig) -> Result<(Backend, EncoderBackend), VideoError> {
		let encoder = software::SoftwareEncoder::new(*config)?;
		Ok((Backend::OpenH264(Box::new(encoder)), EncoderBackend::OpenH264))
	}

	pub fn backend(&self) -> EncoderBackend {
		self.kind
	}

	pub fn config(&self) -> EncoderConfig {
		self.config
	}

	pub fn request_keyframe(&mut self) {
		self.keyframe_requested = true;
	}

	pub fn set_bitrate(&mut self, bitrate: u32) -> Result<(), VideoError> {
		if bitrate == self.config.bitrate {
			return Ok(());
		}
		match &mut self.backend {
			#[cfg(windows)]
			Backend::MediaFoundation(encoder) => encoder.set_bitrate(bitrate)?,
			Backend::OpenH264(encoder) => {
				encoder.set_bitrate(bitrate)?;
				self.keyframe_requested = true;
			}
		}
		self.config.bitrate = bitrate;
		Ok(())
	}

	pub fn encode(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedFrame>, VideoError> {
		let width = self.config.width as usize;
		let height = self.config.height as usize;
		convert::to_nv12(frame, width, height, &mut self.scratch)?;
		let keyframe = std::mem::take(&mut self.keyframe_requested);
		match &mut self.backend {
			#[cfg(windows)]
			Backend::MediaFoundation(encoder) => encoder.encode(&self.scratch, frame.timestamp, keyframe),
			Backend::OpenH264(encoder) => encoder.encode(&self.scratch, keyframe),
		}
	}
}
