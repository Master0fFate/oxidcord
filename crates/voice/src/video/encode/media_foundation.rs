use std::time::{Duration, Instant};

use windows::Win32::Media::MediaFoundation::{
	CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVGOPSize,
	CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode, ICodecAPI, IMFMediaEventGenerator,
	IMFSample, IMFTransform, METransformHaveOutput, METransformNeedInput, MF_E_NO_EVENTS_AVAILABLE,
	MF_E_NOTACCEPTING, MF_EVENT_FLAG_NO_WAIT, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
	MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG_SEQUENCE_HEADER,
	MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_TRANSFORM_ASYNC,
	MF_TRANSFORM_ASYNC_UNLOCK, MFMediaType_Video, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
	MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_FLUSH,
	MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
	MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
	MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
	eAVEncCommonRateControlMode_LowDelayVBR, eAVEncH264VProfile_ConstrainedBase,
};
use windows::core::Interface;

use super::{EncodedFrame, EncoderConfig};
use crate::video::VideoError;
use crate::video::{mf, nal};

const EVENT_POLL: Duration = Duration::from_millis(1);
const EVENT_DEADLINE: Duration = Duration::from_millis(150);
const NEED_INPUT: i32 = METransformNeedInput.0;
const HAVE_OUTPUT: i32 = METransformHaveOutput.0;

pub struct MediaFoundationEncoder {
	transform: IMFTransform,
	codec: Option<ICodecAPI>,
	events: Option<IMFMediaEventGenerator>,
	config: EncoderConfig,
	system_allocates_output: bool,
	output_bytes: u32,
	sequence_header: Option<Vec<u8>>,
	input_credits: u32,
}

unsafe impl Send for MediaFoundationEncoder {}

fn failure(error: windows::core::Error) -> VideoError {
	VideoError::Encode(error.message())
}

impl MediaFoundationEncoder {
	pub fn new(config: EncoderConfig, hardware: bool) -> Result<Self, VideoError> {
		mf::prepare()?;
		let flags: MFT_ENUM_FLAG = MFT_ENUM_FLAG_SORTANDFILTER
			| if hardware {
				MFT_ENUM_FLAG_HARDWARE
			} else {
				MFT_ENUM_FLAG_SYNCMFT
			};
		let input = MFT_REGISTER_TYPE_INFO {
			guidMajorType: MFMediaType_Video,
			guidSubtype: MFVideoFormat_NV12,
		};
		let output = MFT_REGISTER_TYPE_INFO {
			guidMajorType: MFMediaType_Video,
			guidSubtype: MFVideoFormat_H264,
		};
		let mut last = VideoError::NotFound;
		for activate in mf::enumerate(MFT_CATEGORY_VIDEO_ENCODER, flags, Some(input), Some(output))? {
			let transform = match unsafe { activate.ActivateObject::<IMFTransform>() } {
				Ok(transform) => transform,
				Err(error) => {
					last = failure(error);
					continue;
				}
			};
			match Self::configure(transform, config) {
				Ok(encoder) => return Ok(encoder),
				Err(error) => last = error,
			}
		}
		Err(last)
	}

	fn configure(transform: IMFTransform, config: EncoderConfig) -> Result<Self, VideoError> {
		let events = unlock_async(&transform)?;
		Self::negotiate(&transform, &config)?;
		let codec = transform.cast::<ICodecAPI>().ok();
		if let Some(codec) = &codec {
			apply_codec_settings(codec, &config);
		}
		let info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(failure)?;
		let provides = info.dwFlags
			& (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0) as u32
			!= 0;
		unsafe {
			transform
				.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
				.map_err(failure)?;
			transform
				.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
				.map_err(failure)?;
		}
		Ok(Self {
			transform,
			codec,
			events,
			config,
			system_allocates_output: provides,
			output_bytes: info.cbSize.max(config.width * config.height),
			sequence_header: None,
			input_credits: 0,
		})
	}

	fn negotiate(transform: &IMFTransform, config: &EncoderConfig) -> Result<(), VideoError> {
		unsafe {
			let output = mf::media_type().map_err(failure)?;
			output
				.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
				.map_err(failure)?;
			output
				.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
				.map_err(failure)?;
			output
				.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)
				.map_err(failure)?;
			output
				.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
				.map_err(failure)?;
			output
				.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_ConstrainedBase.0 as u32)
				.map_err(failure)?;
			mf::set_size(&output, &MF_MT_FRAME_SIZE, config.width, config.height).map_err(failure)?;
			mf::set_size(&output, &MF_MT_FRAME_RATE, config.fps, 1).map_err(failure)?;
			mf::set_size(&output, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(failure)?;
			transform.SetOutputType(0, &output, 0).map_err(failure)?;
			let input = mf::media_type().map_err(failure)?;
			input
				.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
				.map_err(failure)?;
			input
				.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
				.map_err(failure)?;
			input
				.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
				.map_err(failure)?;
			input
				.SetUINT32(&MF_MT_DEFAULT_STRIDE, config.width)
				.map_err(failure)?;
			mf::set_size(&input, &MF_MT_FRAME_SIZE, config.width, config.height).map_err(failure)?;
			mf::set_size(&input, &MF_MT_FRAME_RATE, config.fps, 1).map_err(failure)?;
			mf::set_size(&input, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(failure)?;
			transform.SetInputType(0, &input, 0).map_err(failure)?;
		}
		Ok(())
	}

	pub fn set_bitrate(&mut self, bitrate: u32) -> Result<(), VideoError> {
		if let Some(codec) = &self.codec {
			unsafe { codec.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &mf::variant_u32(bitrate)) }
				.map_err(failure)?;
		}
		self.config.bitrate = bitrate;
		Ok(())
	}

	pub fn encode(
		&mut self,
		nv12: &[u8],
		timestamp: Duration,
		keyframe: bool,
	) -> Result<Vec<EncodedFrame>, VideoError> {
		mf::prepare()?;
		if keyframe && let Some(codec) = &self.codec {
			unsafe { codec.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &mf::variant_u32(1)) }
				.map_err(failure)?;
		}
		let sample =
			mf::sample_from(nv12, timestamp, mf::frame_duration(self.config.fps)).map_err(failure)?;
		let mut outputs = Vec::new();
		if self.events.is_some() {
			self.encode_async(&sample, &mut outputs)?;
		} else {
			self.encode_sync(&sample, &mut outputs)?;
		}
		Ok(outputs.into_iter().map(|data| self.finish(data)).collect())
	}

	fn finish(&mut self, mut data: Vec<u8>) -> EncodedFrame {
		let keyframe = nal::is_keyframe(&data);
		if keyframe && !has_parameter_sets(&data) {
			if self.sequence_header.is_none() {
				self.sequence_header = Some(self.read_sequence_header());
			}
			if let Some(header) = &self.sequence_header {
				data.splice(0..0, header.iter().copied());
			}
		}
		EncodedFrame { data, keyframe }
	}

	fn read_sequence_header(&self) -> Vec<u8> {
		let Ok(output) = (unsafe { self.transform.GetOutputCurrentType(0) }) else {
			return Vec::new();
		};
		let Ok(size) = (unsafe { output.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }) else {
			return Vec::new();
		};
		let mut header = vec![0u8; size as usize];
		match unsafe { output.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut header, None) } {
			Ok(()) => header,
			Err(_) => Vec::new(),
		}
	}

	fn encode_sync(&mut self, sample: &IMFSample, outputs: &mut Vec<Vec<u8>>) -> Result<(), VideoError> {
		match unsafe { self.transform.ProcessInput(0, sample, 0) } {
			Ok(()) => {}
			Err(error) if error.code() == MF_E_NOTACCEPTING => {
				self.drain(outputs)?;
				unsafe { self.transform.ProcessInput(0, sample, 0) }.map_err(failure)?;
			}
			Err(error) => return Err(failure(error)),
		}
		self.drain(outputs)
	}

	fn drain(&mut self, outputs: &mut Vec<Vec<u8>>) -> Result<(), VideoError> {
		loop {
			match self.take_output() {
				Ok(Some(data)) => outputs.push(data),
				Ok(None) => return Ok(()),
				Err(error) => return Err(error),
			}
		}
	}

	fn take_output(&mut self) -> Result<Option<Vec<u8>>, VideoError> {
		let allocate = (!self.system_allocates_output).then_some(self.output_bytes);
		match mf::process_output(&self.transform, allocate).map_err(failure)? {
			mf::Output::Sample(sample) => {
				Ok(Some(mf::read_buffer(&sample).map_err(failure)?).filter(|data| !data.is_empty()))
			}
			mf::Output::NeedInput | mf::Output::StreamChange => Ok(None),
		}
	}

	fn encode_async(&mut self, sample: &IMFSample, outputs: &mut Vec<Vec<u8>>) -> Result<(), VideoError> {
		let started = Instant::now();
		self.pump_events(outputs, false, started)?;
		while self.input_credits == 0 {
			if started.elapsed() > EVENT_DEADLINE {
				return Ok(());
			}
			self.pump_events(outputs, true, started)?;
		}
		unsafe { self.transform.ProcessInput(0, sample, 0) }.map_err(failure)?;
		self.input_credits -= 1;
		let produced = outputs.len();
		while outputs.len() == produced && started.elapsed() < EVENT_DEADLINE {
			self.pump_events(outputs, true, started)?;
		}
		Ok(())
	}

	fn pump_events(
		&mut self,
		outputs: &mut Vec<Vec<u8>>,
		wait: bool,
		started: Instant,
	) -> Result<(), VideoError> {
		let Some(events) = self.events.clone() else {
			return Ok(());
		};
		loop {
			match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
				Ok(event) => {
					let kind = unsafe { event.GetType() }.map_err(failure)? as i32;
					if kind == NEED_INPUT {
						self.input_credits += 1;
					} else if kind == HAVE_OUTPUT
						&& let Some(data) = self.take_output()?
					{
						outputs.push(data);
					}
				}
				Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => {
					if wait && started.elapsed() < EVENT_DEADLINE {
						std::thread::sleep(EVENT_POLL);
					}
					return Ok(());
				}
				Err(error) => return Err(failure(error)),
			}
		}
	}
}

impl Drop for MediaFoundationEncoder {
	fn drop(&mut self) {
		unsafe {
			let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
		}
	}
}

fn unlock_async(transform: &IMFTransform) -> Result<Option<IMFMediaEventGenerator>, VideoError> {
	let Ok(attributes) = (unsafe { transform.GetAttributes() }) else {
		return Ok(None);
	};
	if unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) == 0 {
		return Ok(None);
	}
	unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }.map_err(failure)?;
	Ok(Some(transform.cast::<IMFMediaEventGenerator>().map_err(failure)?))
}

fn apply_codec_settings(codec: &ICodecAPI, config: &EncoderConfig) {
	unsafe {
		let _ = codec.SetValue(&CODECAPI_AVLowLatencyMode, &mf::variant_bool(true));
		let _ = codec.SetValue(
			&CODECAPI_AVEncCommonRateControlMode,
			&mf::variant_u32(eAVEncCommonRateControlMode_LowDelayVBR.0 as u32),
		);
		let _ = codec.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &mf::variant_u32(config.bitrate));
		let _ = codec.SetValue(
			&CODECAPI_AVEncMPVGOPSize,
			&mf::variant_u32(config.fps * config.keyframe_seconds),
		);
	}
}

fn has_parameter_sets(frame: &[u8]) -> bool {
	nal::units(frame)
		.iter()
		.any(|unit| nal::unit_type(unit[0]) == nal::SPS)
}
