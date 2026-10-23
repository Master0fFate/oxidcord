pub const AVAILABLE: bool = cfg!(feature = "rnnoise");

#[cfg(feature = "rnnoise")]
pub struct Denoiser {
	state: Box<nnnoiseless::DenoiseState<'static>>,
	input: [f32; nnnoiseless::DenoiseState::FRAME_SIZE],
	output: [f32; nnnoiseless::DenoiseState::FRAME_SIZE],
}

#[cfg(feature = "rnnoise")]
impl Denoiser {
	const PCM_SCALE: f32 = 32768.0;

	pub fn new() -> Self {
		Self {
			state: nnnoiseless::DenoiseState::new(),
			input: [0.0; nnnoiseless::DenoiseState::FRAME_SIZE],
			output: [0.0; nnnoiseless::DenoiseState::FRAME_SIZE],
		}
	}

	pub fn process(&mut self, frame: &mut [f32]) {
		for chunk in frame
			.as_chunks_mut::<{ nnnoiseless::DenoiseState::FRAME_SIZE }>()
			.0
		{
			for (scaled, sample) in self.input.iter_mut().zip(chunk.iter()) {
				*scaled = sample * Self::PCM_SCALE;
			}
			self.state.process_frame(&mut self.output, &self.input);
			for (sample, scaled) in chunk.iter_mut().zip(&self.output) {
				*sample = scaled / Self::PCM_SCALE;
			}
		}
	}
}

#[cfg(not(feature = "rnnoise"))]
pub struct Denoiser;

#[cfg(not(feature = "rnnoise"))]
impl Denoiser {
	pub fn new() -> Self {
		Self
	}

	pub fn process(&mut self, _frame: &mut [f32]) {}
}
