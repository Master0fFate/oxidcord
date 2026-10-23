use std::time::Duration;

use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

use crate::video::convert;
use crate::video::{PixelFormat, VideoError, VideoFrame};

pub struct SoftwareDecoder {
	decoder: Decoder,
}

impl SoftwareDecoder {
	pub fn new() -> Result<Self, VideoError> {
		let decoder = Decoder::new().map_err(|error| VideoError::Decode(error.to_string()))?;
		Ok(Self { decoder })
	}

	pub fn decode(&mut self, frame: &[u8], timestamp: Duration) -> Result<Option<VideoFrame>, VideoError> {
		let decoded = self
			.decoder
			.decode(frame)
			.map_err(|error| VideoError::Decode(error.to_string()))?;
		Ok(decoded.map(|yuv| {
			let (width, height) = yuv.dimensions();
			let width = width & !1;
			let height = height & !1;
			VideoFrame {
				width: width as u32,
				height: height as u32,
				format: PixelFormat::Nv12,
				data: convert::i420_to_nv12(yuv.y(), yuv.u(), yuv.v(), yuv.strides(), width, height),
				timestamp,
			}
		}))
	}
}
