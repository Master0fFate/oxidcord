use std::f32::consts::FRAC_PI_2;

use crate::output::OUTPUT_CHANNELS;

const HOP: usize = 512;
const SEARCH: usize = 256;
const COMPARE_STEP: usize = 4;
const COMPARED: usize = HOP / COMPARE_STEP;

pub struct Stretch {
	speed: f32,
	input: Vec<f32>,
	tail: Vec<f32>,
	rise: Vec<f32>,
	cursor: f64,
	follow: usize,
}

impl Stretch {
	pub fn new(speed: f32) -> Self {
		let rise = (0..HOP)
			.map(|index| (FRAC_PI_2 * index as f32 / HOP as f32).sin().powi(2))
			.collect();
		Self {
			speed,
			input: Vec::new(),
			tail: Vec::new(),
			rise,
			cursor: 0.,
			follow: 0,
		}
	}

	pub fn speed(&self) -> f32 {
		self.speed
	}

	pub fn active(&self) -> bool {
		(self.speed - 1.).abs() > f32::EPSILON
	}

	pub fn set_speed(&mut self, speed: f32) {
		self.speed = speed;
		self.reset();
	}

	pub fn reset(&mut self) {
		self.input.clear();
		self.tail.clear();
		self.cursor = 0.;
		self.follow = 0;
	}

	pub fn process(&mut self, samples: &[f32], output: &mut Vec<f32>) {
		self.input.extend_from_slice(samples);
		let frames = self.input.len() / OUTPUT_CHANNELS;
		let mut nominal = self.cursor.round() as usize;
		while nominal + SEARCH + 2 * HOP <= frames {
			if self.tail.is_empty() {
				self.start(output);
			} else {
				let best = self.best_offset(nominal);
				self.overlap(best, output);
			}
			self.cursor += HOP as f64 * f64::from(self.speed);
			nominal = self.cursor.round() as usize;
		}
		self.discard(nominal);
	}

	fn start(&mut self, output: &mut Vec<f32>) {
		output.extend_from_slice(&self.input[..HOP * OUTPUT_CHANNELS]);
		self.fall_into_tail(0);
		self.follow = HOP;
	}

	fn overlap(&mut self, best: usize, output: &mut Vec<f32>) {
		let source = &self.input[best * OUTPUT_CHANNELS..(best + HOP) * OUTPUT_CHANNELS];
		output.extend(
			self.tail
				.chunks_exact(OUTPUT_CHANNELS)
				.zip(source.chunks_exact(OUTPUT_CHANNELS))
				.zip(&self.rise)
				.flat_map(|((held, fresh), rise)| [held[0] + fresh[0] * rise, held[1] + fresh[1] * rise]),
		);
		self.fall_into_tail(best);
		self.follow = best + HOP;
	}

	fn fall_into_tail(&mut self, from: usize) {
		let start = (from + HOP) * OUTPUT_CHANNELS;
		self.tail.clear();
		self.tail.extend(
			self.input[start..start + HOP * OUTPUT_CHANNELS]
				.chunks_exact(OUTPUT_CHANNELS)
				.zip(&self.rise)
				.flat_map(|(frame, rise)| [frame[0] * (1. - rise), frame[1] * (1. - rise)]),
		);
	}

	fn best_offset(&self, nominal: usize) -> usize {
		let low = nominal.saturating_sub(SEARCH);
		let mut target = [0.; COMPARED];
		for (step, slot) in target.iter_mut().enumerate() {
			*slot = self.mono(self.follow + step * COMPARE_STEP);
		}
		(low..=nominal + SEARCH)
			.map(|start| (start, self.distance(start, &target)))
			.min_by(|left, right| left.1.total_cmp(&right.1))
			.map_or(nominal, |(start, _)| start)
	}

	fn mono(&self, frame: usize) -> f32 {
		let at = frame * OUTPUT_CHANNELS;
		self.input[at] + self.input[at + 1]
	}

	fn distance(&self, start: usize, target: &[f32]) -> f32 {
		target
			.iter()
			.enumerate()
			.map(|(step, wanted)| (self.mono(start + step * COMPARE_STEP) - wanted).abs())
			.sum()
	}

	fn discard(&mut self, nominal: usize) {
		let keep_from = self.follow.min(nominal.saturating_sub(SEARCH));
		self.input.drain(..keep_from * OUTPUT_CHANNELS);
		self.cursor -= keep_from as f64;
		self.follow -= keep_from;
	}
}
