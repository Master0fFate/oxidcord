use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler as _};

use crate::error::{Result, VoiceError};

const CHUNK_DIVISOR: u32 = 100;

pub struct Resampler {
	inner: Option<Fft<f32>>,
	channels: usize,
	pending: Vec<f32>,
	scratch: Vec<f32>,
}

impl Resampler {
	pub fn new(rate_in: u32, rate_out: u32, channels: usize) -> Result<Self> {
		let inner = if rate_in == rate_out {
			None
		} else {
			let chunk = (rate_in / CHUNK_DIVISOR).max(1) as usize;
			Some(
				Fft::new(
					rate_in as usize,
					rate_out as usize,
					chunk,
					channels,
					FixedSync::Input,
				)
				.map_err(|error| VoiceError::Resample(error.to_string()))?,
			)
		};
		let scratch = vec![0.0; inner.as_ref().map_or(0, |fft| fft.output_frames_max() * channels)];
		Ok(Self {
			inner,
			channels,
			pending: Vec::new(),
			scratch,
		})
	}

	pub fn process(&mut self, samples: &[f32], output: &mut Vec<f32>) -> Result<()> {
		let Some(fft) = self.inner.as_mut() else {
			output.extend_from_slice(samples);
			return Ok(());
		};
		self.pending.extend_from_slice(samples);
		loop {
			let frames_in = fft.input_frames_next();
			let samples_in = frames_in * self.channels;
			if self.pending.len() < samples_in {
				return Ok(());
			}
			let input = InterleavedSlice::new(&self.pending[..samples_in], self.channels, frames_in)
				.map_err(|error| VoiceError::Resample(error.to_string()))?;
			let frames_out = fft.output_frames_next();
			let mut produced = InterleavedSlice::new_mut(&mut self.scratch, self.channels, frames_out)
				.map_err(|error| VoiceError::Resample(error.to_string()))?;
			let (_, written) = fft
				.process_into_buffer(&input, &mut produced, None)
				.map_err(|error| VoiceError::Resample(error.to_string()))?;
			output.extend_from_slice(&self.scratch[..written * self.channels]);
			self.pending.drain(..samples_in);
		}
	}
}
