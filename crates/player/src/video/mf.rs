use std::time::Duration;

use voice::video::mf;
use windows::Win32::Media::MediaFoundation::{
	IMFMediaType, IMFSample, IMFTransform, MF_E_NOTACCEPTING, MF_LOW_LATENCY, MF_MT_DEFAULT_STRIDE,
	MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE,
	MFMediaType_Video, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
	MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
	MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
	MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
	MFVideoFormat_NV12, MFVideoInterlace_Progressive,
};
use windows::core::GUID;

use super::convert::{Chroma, Picture};
use crate::error::{PlayerError, Result};

const APERTURE_BYTES: usize = 16;
const APERTURE_AREA_OFFSET: usize = 8;
const FALLBACK_OUTPUT_BYTES: u32 = 16 * 1024 * 1024;

fn failure(error: windows::core::Error) -> PlayerError {
	PlayerError::Video(error.message())
}

pub struct MfDecoder {
	transform: IMFTransform,
	allocate: Option<u32>,
	width: usize,
	height: usize,
	stride: usize,
}

unsafe impl Send for MfDecoder {}

impl MfDecoder {
	pub fn open(subtype: GUID, size: Option<(u32, u32)>) -> Result<Self> {
		mf::prepare().map_err(|error| PlayerError::Video(error.to_string()))?;
		let input = MFT_REGISTER_TYPE_INFO {
			guidMajorType: MFMediaType_Video,
			guidSubtype: subtype,
		};
		let flags = MFT_ENUM_FLAG_SORTANDFILTER | MFT_ENUM_FLAG_SYNCMFT;
		let found = mf::enumerate(MFT_CATEGORY_VIDEO_DECODER, flags, Some(input), None)
			.map_err(|error| PlayerError::Video(error.to_string()))?;
		let mut last = PlayerError::Unsupported("no media foundation decoder".to_owned());
		for activate in found {
			let transform = match unsafe { activate.ActivateObject::<IMFTransform>() } {
				Ok(transform) => transform,
				Err(error) => {
					last = failure(error);
					continue;
				}
			};
			match Self::configure(transform, subtype, size) {
				Ok(decoder) => return Ok(decoder),
				Err(error) => last = error,
			}
		}
		Err(last)
	}

	fn configure(transform: IMFTransform, subtype: GUID, size: Option<(u32, u32)>) -> Result<Self> {
		unsafe {
			if let Ok(attributes) = transform.GetAttributes() {
				let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
			}
			let input = mf::media_type().map_err(failure)?;
			input
				.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
				.map_err(failure)?;
			input.SetGUID(&MF_MT_SUBTYPE, &subtype).map_err(failure)?;
			input
				.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
				.map_err(failure)?;
			if let Some((width, height)) = size {
				mf::set_size(&input.clone().into(), &MF_MT_FRAME_SIZE, width, height).map_err(failure)?;
			}
			transform.SetInputType(0, &input, 0).map_err(failure)?;
		}
		let mut decoder = Self {
			transform,
			allocate: None,
			width: 0,
			height: 0,
			stride: 0,
		};
		decoder.select_output()?;
		unsafe {
			decoder
				.transform
				.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
				.map_err(failure)?;
			decoder
				.transform
				.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
				.map_err(failure)?;
		}
		Ok(decoder)
	}

	fn select_output(&mut self) -> Result<()> {
		let mut index = 0;
		let chosen: IMFMediaType = loop {
			let candidate = unsafe { self.transform.GetOutputAvailableType(0, index) }.map_err(failure)?;
			if unsafe { candidate.GetGUID(&MF_MT_SUBTYPE) }.is_ok_and(|subtype| subtype == MFVideoFormat_NV12)
			{
				break candidate;
			}
			index += 1;
		};
		unsafe { self.transform.SetOutputType(0, &chosen, 0) }.map_err(failure)?;
		let (width, height) = display_size(&chosen);
		let stride =
			unsafe { chosen.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.map_or(width, |stride| stride as i32 as usize);
		self.width = width;
		self.height = height;
		self.stride = stride.max(width);
		let info = unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(failure)?;
		let provides = info.dwFlags
			& (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0) as u32
			!= 0;
		let bytes = if info.cbSize == 0 {
			FALLBACK_OUTPUT_BYTES
		} else {
			info.cbSize
		};
		self.allocate = (!provides).then_some(bytes);
		Ok(())
	}

	pub fn decode(&mut self, data: &[u8], pts: Duration, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		let sample = mf::sample_from(data, pts, Duration::ZERO).map_err(failure)?;
		match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
			Ok(()) => {}
			Err(error) if error.code() == MF_E_NOTACCEPTING => {
				self.drain_outputs(sink)?;
				unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(failure)?;
			}
			Err(error) => return Err(failure(error)),
		}
		self.drain_outputs(sink)
	}

	pub fn finish(&mut self, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		unsafe {
			self.transform
				.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
				.map_err(failure)?;
			self.transform
				.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
				.map_err(failure)?;
		}
		self.drain_outputs(sink)
	}

	pub fn reset(&mut self) {
		unsafe {
			let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
			let _ = self
				.transform
				.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
		}
	}

	fn drain_outputs(&mut self, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		loop {
			match mf::process_output(&self.transform, self.allocate).map_err(failure)? {
				mf::Output::Sample(sample) => self.emit(&sample, sink)?,
				mf::Output::NeedInput => return Ok(()),
				mf::Output::StreamChange => self.select_output()?,
			}
		}
	}

	fn emit(&self, sample: &IMFSample, sink: &mut dyn FnMut(Picture)) -> Result<()> {
		let pts = unsafe { sample.GetSampleTime() }
			.map(|ticks| Duration::from_nanos(ticks.max(0) as u64 * 100))
			.map_err(failure)?;
		let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(failure)?;
		let mut data = std::ptr::null_mut();
		let mut length = 0u32;
		unsafe { buffer.Lock(&mut data, None, Some(&mut length)) }.map_err(failure)?;
		let bytes = unsafe { std::slice::from_raw_parts(data, length as usize) };
		let (width, height, stride) = (self.width, self.height, self.stride);
		let aligned_height = (bytes.len() * 2 / (3 * stride.max(1))).max(height);
		if width > 0 && height > 0 && bytes.len() >= stride * aligned_height * 3 / 2 {
			let (luma, uv) = bytes.split_at(stride * aligned_height);
			sink(Picture {
				width,
				height,
				luma,
				luma_stride: stride,
				chroma: Chroma::Interleaved { uv, stride },
				wide: false,
				pts,
			});
		}
		unsafe { buffer.Unlock() }.map_err(failure)?;
		Ok(())
	}
}

fn display_size(media_type: &IMFMediaType) -> (usize, usize) {
	let mut aperture = [0u8; APERTURE_BYTES];
	if unsafe { media_type.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut aperture, None) }.is_ok() {
		let read = |at: usize| {
			i32::from_le_bytes([aperture[at], aperture[at + 1], aperture[at + 2], aperture[at + 3]])
		};
		let (width, height) = (read(APERTURE_AREA_OFFSET), read(APERTURE_AREA_OFFSET + 4));
		if width > 0 && height > 0 {
			return (width as usize & !1, height as usize & !1);
		}
	}
	let (width, height) = mf::size_of(&media_type.clone().into(), &MF_MT_FRAME_SIZE).unwrap_or_default();
	(width as usize & !1, height as usize & !1)
}
