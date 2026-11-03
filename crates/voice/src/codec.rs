use opus::{Application, Bitrate, Channels, Decoder, Encoder};

use crate::error::Result;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960;
pub const STEREO_FRAME_SAMPLES: usize = FRAME_SAMPLES * 2;
pub const SILENCE_FRAME: [u8; 3] = [0xf8, 0xff, 0xfe];

pub const DEFAULT_BITRATE_BPS: u32 = 64_000;
pub const STREAM_BITRATE_BPS: u32 = 128_000;
pub const MIN_BITRATE_BPS: u32 = 8_000;
pub const MAX_BITRATE_BPS: u32 = 384_000;
const EXPECTED_LOSS_PERCENT: i32 = 10;
const MAX_PACKET_BYTES: usize = 1275;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
	Voice,
	Stream,
}

impl Profile {
	fn channels(self) -> Channels {
		match self {
			Self::Voice => Channels::Mono,
			Self::Stream => Channels::Stereo,
		}
	}

	fn application(self) -> Application {
		match self {
			Self::Voice => Application::Voip,
			Self::Stream => Application::Audio,
		}
	}

	pub fn bitrate_bps(self) -> u32 {
		match self {
			Self::Voice => DEFAULT_BITRATE_BPS,
			Self::Stream => STREAM_BITRATE_BPS,
		}
	}
}

pub struct VoiceEncoder {
	inner: Encoder,
	packet: [u8; MAX_PACKET_BYTES],
}

impl VoiceEncoder {
	pub fn new(profile: Profile) -> Result<Self> {
		let mut inner = Encoder::new(SAMPLE_RATE, profile.channels(), profile.application())?;
		inner.set_bitrate(Bitrate::Bits(profile.bitrate_bps() as i32))?;
		inner.set_inband_fec(true)?;
		inner.set_packet_loss_perc(EXPECTED_LOSS_PERCENT)?;
		inner.set_dtx(profile == Profile::Voice)?;
		Ok(Self {
			inner,
			packet: [0; MAX_PACKET_BYTES],
		})
	}

	pub fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<()> {
		let clamped = bitrate_bps.clamp(MIN_BITRATE_BPS, MAX_BITRATE_BPS);
		self.inner.set_bitrate(Bitrate::Bits(clamped as i32))?;
		Ok(())
	}

	pub fn encode(&mut self, pcm: &[f32]) -> Result<&[u8]> {
		let length = self.inner.encode_float(pcm, &mut self.packet)?;
		Ok(&self.packet[..length])
	}
}

pub struct VoiceDecoder {
	inner: Decoder,
}

impl VoiceDecoder {
	pub fn new() -> Result<Self> {
		Ok(Self {
			inner: Decoder::new(SAMPLE_RATE, Channels::Stereo)?,
		})
	}

	pub fn decode(&mut self, packet: &[u8], recover_previous: bool, stereo: &mut [f32]) -> Result<()> {
		let samples = self.inner.decode_float(packet, stereo, recover_previous)? * 2;
		stereo[samples..].fill(0.0);
		Ok(())
	}
}
