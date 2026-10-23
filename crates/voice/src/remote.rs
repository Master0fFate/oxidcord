use std::time::Instant;

use tracing::debug;

use crate::codec::{STEREO_FRAME_SAMPLES, VoiceDecoder};
use crate::error::Result;
use crate::jitter::{JitterBuffer, Playout, Push};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Frame {
	Silence,
	Decoded,
	Concealed,
}

pub(crate) struct RemoteStream {
	decoder: VoiceDecoder,
	jitter: JitterBuffer,
	pcm: Vec<f32>,
	last_packet: Instant,
}

impl RemoteStream {
	pub fn new(now: Instant) -> Result<Self> {
		Ok(Self {
			decoder: VoiceDecoder::new()?,
			jitter: JitterBuffer::new(now),
			pcm: vec![0.0; STEREO_FRAME_SAMPLES],
			last_packet: now,
		})
	}

	pub fn push(&mut self, sequence: u16, timestamp: u32, payload: Vec<u8>, now: Instant) -> Push {
		self.last_packet = now;
		self.jitter.push(sequence, timestamp, payload, now)
	}

	pub fn next_frame(&mut self) -> Frame {
		match self.jitter.pop() {
			Playout::Silence => Frame::Silence,
			Playout::Packet(packet) => match self.decoder.decode(&packet, false, &mut self.pcm) {
				Ok(()) => Frame::Decoded,
				Err(error) => {
					debug!(%error, "dropping undecodable opus packet");
					self.conceal(&[], false)
				}
			},
			Playout::Missing { recovery: Some(next) } => self.conceal(&next, true),
			Playout::Missing { recovery: None } => self.conceal(&[], false),
		}
	}

	pub fn pcm(&self) -> &[f32] {
		&self.pcm
	}

	pub fn jitter_ms(&self) -> f32 {
		self.jitter.jitter_ms()
	}

	pub fn idle_since(&self, now: Instant) -> std::time::Duration {
		now.duration_since(self.last_packet)
	}

	fn conceal(&mut self, packet: &[u8], recover_previous: bool) -> Frame {
		match self.decoder.decode(packet, recover_previous, &mut self.pcm) {
			Ok(()) => Frame::Concealed,
			Err(error) => {
				debug!(%error, "opus concealment failed");
				Frame::Silence
			}
		}
	}
}
