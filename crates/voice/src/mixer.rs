const KNEE: f32 = 0.75;
const HEADROOM: f32 = 1.0 - KNEE;

pub struct Mixer {
	accumulator: Vec<f32>,
}

impl Mixer {
	pub fn new(samples: usize) -> Self {
		Self {
			accumulator: vec![0.0; samples],
		}
	}

	pub fn clear(&mut self) {
		self.accumulator.fill(0.0);
	}

	pub fn add(&mut self, frame: &[f32], gain: f32) {
		for (sum, sample) in self.accumulator.iter_mut().zip(frame) {
			*sum += sample * gain;
		}
	}

	pub fn finish(&mut self, master: f32) -> &[f32] {
		for sample in &mut self.accumulator {
			*sample = soft_clip(*sample * master);
		}
		&self.accumulator
	}

	pub fn output(&self) -> &[f32] {
		&self.accumulator
	}
}

pub fn soft_clip(sample: f32) -> f32 {
	let magnitude = sample.abs();
	if magnitude <= KNEE {
		sample
	} else {
		sample.signum() * (KNEE + HEADROOM * ((magnitude - KNEE) / HEADROOM).tanh())
	}
}
