use std::sync::Arc;

use crate::audio::Rings;
use crate::codec::{FRAME_SAMPLES, SAMPLE_RATE};
use crate::error::Result;
use crate::resample::Resampler;

const PLAYBACK_BUFFER_MS: usize = 60;

const SYSTEM_PENDING_FRAMES: usize = 5;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Source {
	#[default]
	Microphone,
	System,
}

impl Source {
	fn channels(self) -> usize {
		match self {
			Self::Microphone => 1,
			Self::System => 2,
		}
	}
}

#[derive(Default)]
pub(crate) struct CaptureChain {
	source: Source,
	rate: u32,
	resampler: Option<Resampler>,
	raw: Vec<f32>,
	pending: Vec<f32>,
}

impl CaptureChain {
	pub fn system() -> Self {
		Self {
			source: Source::System,
			..Self::default()
		}
	}

	pub fn pull(&mut self, rings: &Rings) -> Result<()> {
		let rate = match self.source {
			Source::Microphone => rings.capture_rate(),
			Source::System => rings.system_rate(),
		};
		if rate != self.rate {
			self.rate = rate;
			self.resampler = (rate != 0)
				.then(|| Resampler::new(rate, SAMPLE_RATE, self.source.channels()))
				.transpose()?;
			self.pending.clear();
		}
		let Some(resampler) = self.resampler.as_mut() else {
			return Ok(());
		};
		self.raw.clear();
		match self.source {
			Source::Microphone => rings.drain_capture(&mut self.raw),
			Source::System => rings.drain_system(&mut self.raw),
		}
		resampler.process(&self.raw, &mut self.pending)?;
		if self.source == Source::System {
			let excess = self
				.pending
				.len()
				.saturating_sub(FRAME_SAMPLES * self.source.channels() * SYSTEM_PENDING_FRAMES);
			self.pending.drain(..excess);
		}
		Ok(())
	}

	#[cfg(feature = "testing")]
	pub fn inject(&mut self, frame: &[f32]) {
		self.pending.extend_from_slice(frame);
	}

	pub fn backlog(&self) -> usize {
		self.pending.len() / self.source.channels()
	}

	pub fn take_frame(&mut self, frame: &mut [f32]) -> bool {
		if self.pending.len() < frame.len() {
			return false;
		}
		frame.copy_from_slice(&self.pending[..frame.len()]);
		self.pending.drain(..frame.len());
		true
	}
}

pub(crate) struct PlaybackChain {
	rings: Arc<Rings>,
	rate: u32,
	resampler: Option<Resampler>,
	converted: Vec<f32>,
}

impl PlaybackChain {
	pub fn new(rings: Arc<Rings>) -> Self {
		Self {
			rings,
			rate: 0,
			resampler: None,
			converted: Vec::new(),
		}
	}

	pub fn has_device(&self) -> bool {
		self.rings.playback_rate() != 0
	}

	pub fn wants_frame(&self) -> bool {
		let rate = self.rings.playback_rate() as usize;
		rate != 0 && self.rings.playback_len() < rate * PLAYBACK_BUFFER_MS / 1000 * 2
	}

	pub fn push(&mut self, stereo: &[f32]) -> Result<()> {
		let rate = self.rings.playback_rate();
		if rate == 0 {
			return Ok(());
		}
		if rate != self.rate {
			self.rate = rate;
			self.resampler = Some(Resampler::new(SAMPLE_RATE, rate, 2)?);
		}
		let Some(resampler) = self.resampler.as_mut() else {
			return Ok(());
		};
		self.converted.clear();
		resampler.process(stereo, &mut self.converted)?;
		self.rings.push_playback(&self.converted);
		Ok(())
	}
}
