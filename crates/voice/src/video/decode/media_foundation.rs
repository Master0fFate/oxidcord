use std::time::Duration;

use windows::Win32::Media::MediaFoundation::{
	IMFMediaType, IMFTransform, MF_E_NOTACCEPTING, MF_LOW_LATENCY, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
	MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE, MFMediaType_Video,
	MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
	MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
	MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
	MFVideoFormat_H264, MFVideoFormat_NV12,
};

use crate::video::{PixelFormat, VideoError, VideoFrame, mf};

const APERTURE_BYTES: usize = 16;
const APERTURE_AREA_OFFSET: usize = 8;
const FALLBACK_OUTPUT_BYTES: u32 = 8 * 1024 * 1024;

struct OutputLayout {
	width: usize,
	height: usize,
	stride: usize,
}

pub struct MediaFoundationDecoder {
	transform: IMFTransform,
	layout: OutputLayout,
	allocate_bytes: Option<u32>,
}

unsafe impl Send for MediaFoundationDecoder {}

fn failure(error: windows::core::Error) -> VideoError {
	VideoError::Decode(error.message())
}

impl MediaFoundationDecoder {
	pub fn new() -> Result<Self, VideoError> {
		mf::prepare()?;
		let input = MFT_REGISTER_TYPE_INFO {
			guidMajorType: MFMediaType_Video,
			guidSubtype: MFVideoFormat_H264,
		};
		let mut last = VideoError::NotFound;
		let flags = MFT_ENUM_FLAG_SORTANDFILTER | MFT_ENUM_FLAG_SYNCMFT;
		for activate in mf::enumerate(MFT_CATEGORY_VIDEO_DECODER, flags, Some(input), None)? {
			let transform = match unsafe { activate.ActivateObject::<IMFTransform>() } {
				Ok(transform) => transform,
				Err(error) => {
					last = failure(error);
					continue;
				}
			};
			match Self::configure(transform) {
				Ok(decoder) => return Ok(decoder),
				Err(error) => last = error,
			}
		}
		Err(last)
	}

	fn configure(transform: IMFTransform) -> Result<Self, VideoError> {
		unsafe {
			if let Ok(attributes) = transform.GetAttributes() {
				let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
			}
			let input = mf::media_type().map_err(failure)?;
			input
				.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
				.map_err(failure)?;
			input
				.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
				.map_err(failure)?;
			transform.SetInputType(0, &input, 0).map_err(failure)?;
		}
		let mut decoder = Self {
			transform,
			layout: OutputLayout {
				width: 0,
				height: 0,
				stride: 0,
			},
			allocate_bytes: None,
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

	fn select_output(&mut self) -> Result<(), VideoError> {
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
		self.layout = OutputLayout {
			width,
			height,
			stride: stride.max(width),
		};
		let info = unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(failure)?;
		let provides = info.dwFlags
			& (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0) as u32
			!= 0;
		let bytes = if info.cbSize == 0 {
			FALLBACK_OUTPUT_BYTES
		} else {
			info.cbSize
		};
		self.allocate_bytes = (!provides).then_some(bytes);
		Ok(())
	}

	pub fn decode(&mut self, frame: &[u8], timestamp: Duration) -> Result<Option<VideoFrame>, VideoError> {
		mf::prepare()?;
		let sample = mf::sample_from(frame, timestamp, Duration::ZERO).map_err(failure)?;
		let mut latest = None;
		match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
			Ok(()) => {}
			Err(error) if error.code() == MF_E_NOTACCEPTING => {
				self.drain(timestamp, &mut latest)?;
				unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(failure)?;
			}
			Err(error) => return Err(failure(error)),
		}
		self.drain(timestamp, &mut latest)?;
		Ok(latest)
	}

	fn drain(&mut self, timestamp: Duration, latest: &mut Option<VideoFrame>) -> Result<(), VideoError> {
		loop {
			match mf::process_output(&self.transform, self.allocate_bytes).map_err(failure)? {
				mf::Output::Sample(sample) => *latest = Some(self.convert(&sample, timestamp)?),
				mf::Output::NeedInput => return Ok(()),
				mf::Output::StreamChange => self.select_output()?,
			}
		}
	}

	fn convert(
		&self,
		sample: &windows::Win32::Media::MediaFoundation::IMFSample,
		timestamp: Duration,
	) -> Result<VideoFrame, VideoError> {
		let data = mf::read_buffer(sample).map_err(failure)?;
		let OutputLayout {
			width,
			height,
			stride,
		} = self.layout;
		let aligned_height = (data.len() * 2 / (3 * stride)).max(height);
		if width == 0 || height == 0 || data.len() < stride * aligned_height * 3 / 2 {
			return Err(VideoError::Decode("decoder returned a short frame".to_owned()));
		}
		let (luma, chroma) = data.split_at(stride * aligned_height);
		let mut packed = Vec::with_capacity(width * height * 3 / 2);
		for row in 0..height {
			packed.extend_from_slice(&luma[row * stride..][..width]);
		}
		for row in 0..height / 2 {
			packed.extend_from_slice(&chroma[row * stride..][..width]);
		}
		Ok(VideoFrame {
			width: width as u32,
			height: height as u32,
			format: PixelFormat::Nv12,
			data: packed,
			timestamp,
		})
	}
}

impl Drop for MediaFoundationDecoder {
	fn drop(&mut self) {
		unsafe {
			let _ = self.transform.ProcessMessage(
				windows::Win32::Media::MediaFoundation::MFT_MESSAGE_COMMAND_FLUSH,
				0,
			);
		}
	}
}

fn display_size(media_type: &IMFMediaType) -> (usize, usize) {
	let mut aperture = [0u8; APERTURE_BYTES];
	if unsafe { media_type.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut aperture, None) }.is_ok() {
		let width = i32::from_le_bytes(
			aperture[APERTURE_AREA_OFFSET..APERTURE_AREA_OFFSET + 4]
				.try_into()
				.unwrap_or_default(),
		);
		let height = i32::from_le_bytes(
			aperture[APERTURE_AREA_OFFSET + 4..]
				.try_into()
				.unwrap_or_default(),
		);
		if width > 0 && height > 0 {
			return (width as usize & !1, height as usize & !1);
		}
	}
	let (width, height) = mf::size_of(&media_type.clone().into(), &MF_MT_FRAME_SIZE).unwrap_or_default();
	(width as usize & !1, height as usize & !1)
}
