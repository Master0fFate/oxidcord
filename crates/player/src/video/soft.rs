use std::time::Duration;

use openh264::decoder::{DecodedYUV, Decoder};
use openh264::formats::YUVSource;

use super::convert::{Chroma, Picture};
use crate::error::{PlayerError, Result};

pub struct SoftDecoder {
	decoder: Decoder,
}

fn emit(yuv: &DecodedYUV, sink: &mut dyn FnMut(Picture)) {
	let (width, height) = yuv.dimensions();
	let (luma_stride, chroma_stride, _) = yuv.strides();
	sink(Picture {
		width: width & !1,
		height: height & !1,
		luma: yuv.y(),
		luma_stride,
		chroma: Chroma::Planar {
			u: yuv.u(),
			v: yuv.v(),
			stride: chroma_stride,
		},
		wide: false,
		pts: Duration::ZERO,
	});
}

fn failure(error: openh264::Error) -> PlayerError {
	PlayerError::Video(error.to_string())
}

impl SoftDecoder {
	pub fn new() -> Result<Self> {
		Ok(Self {
			decoder: Decoder::new().map_err(failure)?,
		})
	}

	pub fn decode(&mut self, data: &[u8], sink: &mut dyn FnMut(Picture)) -> Result<()> {
		if let Some(yuv) = self.decoder.decode(data).map_err(failure)? {
			emit(&yuv, sink);
		}
		Ok(())
	}

	pub fn finish(&mut self, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		for yuv in &self.decoder.flush_remaining().map_err(failure)? {
			emit(yuv, sink);
		}
		Ok(())
	}

	pub fn reset(&mut self) -> Result<()> {
		self.decoder = Decoder::new().map_err(failure)?;
		Ok(())
	}
}
