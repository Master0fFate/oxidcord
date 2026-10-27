mod bitstream;
mod convert;
mod ffmpeg;
#[cfg(windows)]
mod mf;
mod soft;

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::CStr;
use std::time::Duration;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::video::well_known::extra_data::{
	VIDEO_EXTRA_DATA_ID_AV1_DECODER_CONFIG, VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG,
	VIDEO_EXTRA_DATA_ID_HEVC_DECODER_CONFIG,
};
use symphonia::core::codecs::video::well_known::{
	CODEC_ID_AV1, CODEC_ID_H264, CODEC_ID_HEVC, CODEC_ID_VP8, CODEC_ID_VP9,
};
use symphonia::core::codecs::video::{VideoCodecParameters, VideoExtraDataId};

use self::bitstream::Bitstream;
pub use self::convert::Picture;
pub use self::ffmpeg::available as codec_pack_available;
use crate::error::{PlayerError, Result};

enum Backend {
	Pack(ffmpeg::FfDecoder),
	#[cfg(windows)]
	Native(mf::MfDecoder),
	Software(soft::SoftDecoder),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
	Avc,
	Hevc,
	Vp8,
	Vp9,
	Av1,
}

impl Codec {
	fn pack_decoders(self) -> &'static [&'static CStr] {
		match self {
			Self::Avc => &[c"h264"],
			Self::Hevc => &[c"hevc"],
			Self::Vp8 => &[c"vp8"],
			Self::Vp9 => &[c"vp9"],
			Self::Av1 => &[c"libdav1d", c"libaom-av1"],
		}
	}
}

pub struct VideoTrack {
	backend: Backend,
	codec: Codec,
	bitstream: Option<Bitstream>,
	header: Vec<u8>,
	header_due: bool,
	scratch: Vec<u8>,
	size: Option<(u32, u32)>,
	order: BinaryHeap<Reverse<Duration>>,
}

const AV1_CONFIG_HEADER: usize = 4;

const AV1_SEQUENCE_HEADER: u8 = 1;
const MAX_REORDER: usize = 32;
const VP9_MARKER: u8 = 0b10;

fn extra(params: &VideoCodecParameters, id: VideoExtraDataId) -> Option<&[u8]> {
	params
		.extra_data
		.iter()
		.find(|extra| extra.id == id)
		.map(|extra| &*extra.data)
}

impl VideoTrack {
	pub fn open(params: &CodecParameters) -> Result<Self> {
		let CodecParameters::Video(params) = params else {
			return Err(PlayerError::Unsupported("not a video track".to_owned()));
		};
		let size = params
			.width
			.zip(params.height)
			.map(|(width, height)| (u32::from(width), u32::from(height)));
		let codec = match params.codec {
			CODEC_ID_H264 => Codec::Avc,
			CODEC_ID_HEVC => Codec::Hevc,
			CODEC_ID_VP8 => Codec::Vp8,
			CODEC_ID_VP9 => Codec::Vp9,
			CODEC_ID_AV1 => Codec::Av1,
			other => return Err(PlayerError::Unsupported(format!("video codec {other}"))),
		};
		let bitstream = match codec {
			Codec::Avc => extra(params, VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG).and_then(Bitstream::avc),
			Codec::Hevc => extra(params, VIDEO_EXTRA_DATA_ID_HEVC_DECODER_CONFIG).and_then(Bitstream::hevc),
			_ => None,
		};
		let header = match codec {
			Codec::Av1 => extra(params, VIDEO_EXTRA_DATA_ID_AV1_DECODER_CONFIG)
				.and_then(|config| config.get(AV1_CONFIG_HEADER..))
				.map(<[u8]>::to_vec)
				.unwrap_or_default(),
			_ => Vec::new(),
		};
		Ok(Self {
			backend: Self::backend(codec, size)?,
			codec,
			bitstream,
			header_due: !header.is_empty(),
			header,
			scratch: Vec::new(),
			size,
			order: BinaryHeap::new(),
		})
	}

	fn reorders(&self) -> bool {
		matches!(self.codec, Codec::Avc | Codec::Hevc)
	}

	pub fn is_keyframe(&self, data: &[u8]) -> bool {
		match (self.codec, &self.bitstream) {
			(Codec::Avc | Codec::Hevc, Some(bitstream)) => bitstream.is_random_access_unit(data),
			(Codec::Avc | Codec::Hevc, None) => true,
			(Codec::Vp8, _) => data.first().is_some_and(|byte| byte & 1 == 0),
			(Codec::Vp9, _) => vp9_keyframe(data),
			(Codec::Av1, _) => av1_keyframe(data),
		}
	}

	fn backend(codec: Codec, size: Option<(u32, u32)>) -> Result<Backend> {
		match ffmpeg::FfDecoder::open(codec.pack_decoders()) {
			Ok(decoder) => Ok(Backend::Pack(decoder)),
			Err(error) => {
				tracing::debug!(%error, "codec pack decoder unavailable");
				Self::platform_backend(codec, size)
			}
		}
	}

	#[cfg(windows)]
	fn platform_backend(codec: Codec, size: Option<(u32, u32)>) -> Result<Backend> {
		use windows::Win32::Media::MediaFoundation::{
			MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_VP80, MFVideoFormat_VP90,
		};
		let subtype = match codec {
			Codec::Avc => MFVideoFormat_H264,
			Codec::Hevc => MFVideoFormat_HEVC,
			Codec::Vp8 => MFVideoFormat_VP80,
			Codec::Vp9 => MFVideoFormat_VP90,
			Codec::Av1 => MFVideoFormat_AV1,
		};
		match mf::MfDecoder::open(subtype, size) {
			Ok(decoder) => Ok(Backend::Native(decoder)),
			Err(error) if codec == Codec::Avc => {
				tracing::debug!(%error, "media foundation decoder unavailable");
				Ok(Backend::Software(soft::SoftDecoder::new()?))
			}
			Err(error) => Err(error),
		}
	}

	#[cfg(not(windows))]
	fn platform_backend(codec: Codec, _: Option<(u32, u32)>) -> Result<Backend> {
		match codec {
			Codec::Avc => Ok(Backend::Software(soft::SoftDecoder::new()?)),
			_ => Err(PlayerError::Unsupported("video codec".to_owned())),
		}
	}

	pub fn size(&self) -> Option<(u32, u32)> {
		self.size
	}

	pub fn decode(&mut self, data: &[u8], pts: Duration, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		let reorders = self.reorders();
		if reorders {
			self.order.push(Reverse(pts));
			if self.order.len() > MAX_REORDER {
				self.order.pop();
			}
		}
		let data = match &mut self.bitstream {
			Some(bitstream) => bitstream.convert(data),
			None if self.header_due => {
				self.header_due = false;
				self.scratch.clear();
				self.scratch.extend_from_slice(&self.header);
				self.scratch.extend_from_slice(data);
				&self.scratch
			}
			None => data,
		};
		let order = &mut self.order;
		let mut timed = |mut picture: Picture| {
			if reorders {
				picture.pts = order.pop().map_or(pts, |Reverse(first)| first);
			}
			sink(picture);
		};
		match &mut self.backend {
			Backend::Pack(decoder) => decoder.decode(data, pts, &mut timed),
			#[cfg(windows)]
			Backend::Native(decoder) => decoder.decode(data, pts, &mut timed),
			Backend::Software(decoder) => decoder.decode(data, &mut timed),
		}
	}

	pub fn finish(&mut self, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		let reorders = self.reorders();
		let order = &mut self.order;
		let mut timed = |mut picture: Picture| {
			if reorders {
				picture.pts = order.pop().map_or(picture.pts, |Reverse(first)| first);
			}
			sink(picture);
		};
		match &mut self.backend {
			Backend::Pack(decoder) => decoder.finish(&mut timed),
			#[cfg(windows)]
			Backend::Native(decoder) => decoder.finish(&mut timed),
			Backend::Software(decoder) => decoder.finish(&mut timed),
		}
	}

	pub fn reset(&mut self) -> Result<()> {
		self.order.clear();
		self.header_due = !self.header.is_empty();
		match &mut self.backend {
			Backend::Pack(decoder) => {
				decoder.reset();
				Ok(())
			}
			#[cfg(windows)]
			Backend::Native(decoder) => {
				decoder.reset();
				Ok(())
			}
			Backend::Software(decoder) => decoder.reset(),
		}
	}
}

fn vp9_keyframe(data: &[u8]) -> bool {
	let Some(&first) = data.first() else {
		return false;
	};
	let bit = |index: u32| (first >> (7 - index)) & 1;
	if first >> 6 != VP9_MARKER {
		return false;
	}
	let profile = bit(2) | (bit(3) << 1);
	let next = if profile == 3 { 5 } else { 4 };
	bit(next) == 0 && bit(next + 1) == 0
}

fn av1_keyframe(data: &[u8]) -> bool {
	let mut at = 0usize;
	while let Some(&header) = data.get(at) {
		let kind = (header >> 3) & 0x0f;
		if kind == AV1_SEQUENCE_HEADER {
			return true;
		}
		let extension = header & 0b100 != 0;
		let has_size = header & 0b10 != 0;
		at += 1 + usize::from(extension);
		if !has_size {
			return false;
		}
		let mut size = 0usize;
		let mut shift = 0;
		loop {
			let Some(&byte) = data.get(at) else {
				return false;
			};
			at += 1;
			size |= usize::from(byte & 0x7f) << shift;
			if byte & 0x80 == 0 {
				break;
			}
			shift += 7;
			if shift > 56 {
				return false;
			}
		}
		at += size;
	}
	false
}
