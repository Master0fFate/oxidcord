use opus::{Application, Bitrate, Channels, Decoder, Encoder};

use crate::error::Result;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960;
pub const STEREO_FRAME_SAMPLES: usize = FRAME_SAMPLES * 2;
pub const SILENCE_FRAME: [u8; 3] = [0xf8, 0xff, 0xfe];

const BITRATE_BPS: i32 = 64_000;
const EXPECTED_LOSS_PERCENT: i32 = 10;
const MAX_PACKET_BYTES: usize = 1275;

pub struct VoiceEncoder {
	inner: Encoder,
	packet: [u8; MAX_PACKET_BYTES],
}

impl VoiceEncoder {
	pub fn new() -> Result<Self> {
		let mut inner = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)?;
		inner.set_bitrate(Bitrate::Bits(BITRATE_BPS))?;
		inner.set_inband_fec(true)?;
		inner.set_packet_loss_perc(EXPECTED_LOSS_PERCENT)?;
		inner.set_dtx(true)?;
		Ok(Self {
			inner,
			packet: [0; MAX_PACKET_BYTES],
		})
	}

	pub fn encode(&mut self, mono: &[f32]) -> Result<&[u8]> {
		let length = self.inner.encode_float(mono, &mut self.packet)?;
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
