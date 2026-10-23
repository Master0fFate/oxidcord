use openh264::OpenH264API;
use openh264::encoder::{
	BitRate, Encoder, EncoderConfig as NativeConfig, FrameRate, IntraFramePeriod, Profile, RateControlMode,
	UsageType,
};
use openh264::formats::YUVBuffer;

use super::{EncodedFrame, EncoderConfig};
use crate::video::convert;
use crate::video::{VideoError, VideoKind};

pub struct SoftwareEncoder {
	encoder: Encoder,
	config: EncoderConfig,
}

fn failure(error: openh264::Error) -> VideoError {
	VideoError::Encode(error.to_string())
}

fn native(config: &EncoderConfig) -> NativeConfig {
	NativeConfig::new()
		.bitrate(BitRate::from_bps(config.bitrate))
		.max_frame_rate(FrameRate::from_hz(config.fps as f32))
		.rate_control_mode(RateControlMode::Bitrate)
		.usage_type(match config.kind {
			VideoKind::Camera => UsageType::CameraVideoRealTime,
			VideoKind::Screen => UsageType::ScreenContentRealTime,
		})
		.profile(Profile::Baseline)
		.intra_frame_period(IntraFramePeriod::from_num_frames(
			config.fps * config.keyframe_seconds,
		))
		.skip_frames(false)
}

impl SoftwareEncoder {
	pub fn new(config: EncoderConfig) -> Result<Self, VideoError> {
		let encoder =
			Encoder::with_api_config(OpenH264API::from_source(), native(&config)).map_err(failure)?;
		Ok(Self { encoder, config })
	}

	pub fn encode(&mut self, nv12: &[u8], keyframe: bool) -> Result<Vec<EncodedFrame>, VideoError> {
		let width = self.config.width as usize;
		let height = self.config.height as usize;
		if keyframe {
			self.encoder.force_intra_frame();
		}
		let source = YUVBuffer::from_vec(convert::nv12_to_i420(nv12, width, height), width, height);
		let bitstream = self.encoder.encode(&source).map_err(failure)?;
		let data = bitstream.to_vec();
		if data.is_empty() {
			return Ok(Vec::new());
		}
		Ok(vec![EncodedFrame {
			keyframe: crate::video::nal::is_keyframe(&data),
			data,
		}])
	}

	pub fn set_bitrate(&mut self, bitrate: u32) -> Result<(), VideoError> {
		self.config.bitrate = bitrate;
		self.encoder =
			Encoder::with_api_config(OpenH264API::from_source(), native(&self.config)).map_err(failure)?;
		Ok(())
	}
}
