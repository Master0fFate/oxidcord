use std::sync::LazyLock;
use std::time::Instant;

use parking_lot::Mutex;

use crate::codec::SAMPLE_RATE;
use crate::error::Result;
use crate::resample::Resampler;

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const TIMELINE_SAMPLES: u64 = SAMPLE_RATE as u64;
const WRITE_TOLERANCE: u64 = millis(30);
const READ_MARGIN: u64 = millis(10);
const MAX_READ_LAG: u64 = millis(60);

static TIMELINE: LazyLock<Mutex<Timeline>> = LazyLock::new(|| Mutex::new(Timeline::new()));

const fn millis(milliseconds: u64) -> u64 {
	milliseconds * SAMPLE_RATE as u64 / 1000
}

fn slot(position: u64) -> usize {
	(position % TIMELINE_SAMPLES) as usize
}

struct Timeline {
	epoch: Instant,
	samples: Box<[f32]>,
	head: u64,
}

impl Timeline {
	fn new() -> Self {
		Self {
			epoch: Instant::now(),
			samples: vec![0.0; TIMELINE_SAMPLES as usize].into_boxed_slice(),
			head: 0,
		}
	}

	fn now(&self) -> u64 {
		(self.epoch.elapsed().as_nanos() * u128::from(SAMPLE_RATE) / NANOS_PER_SECOND) as u64
	}

	fn oldest(&self) -> u64 {
		self.head.saturating_sub(TIMELINE_SAMPLES)
	}

	fn add(&mut self, start: u64, played: &[f32]) {
		let end = start + played.len() as u64;
		if end > self.head {
			for position in self.head.max(end.saturating_sub(TIMELINE_SAMPLES))..end {
				self.samples[slot(position)] = 0.0;
			}
			self.head = end;
		}
		let oldest = self.oldest();
		for (position, sample) in (start..).zip(played) {
			if position >= oldest {
				self.samples[slot(position)] += sample;
			}
		}
	}

	fn copy(&self, start: u64, frame: &mut [f32]) {
		let oldest = self.oldest();
		for (position, sample) in (start..).zip(frame) {
			*sample = if (oldest..self.head).contains(&position) {
				self.samples[slot(position)]
			} else {
				0.0
			};
		}
	}
}

pub struct RenderTap {
	resampler: Resampler,
	converted: Vec<f32>,
	cursor: Option<u64>,
}

impl RenderTap {
	pub fn new(rate: u32) -> Result<Self> {
		Ok(Self {
			resampler: Resampler::new(rate, SAMPLE_RATE, 1)?,
			converted: Vec::new(),
			cursor: None,
		})
	}

	pub fn write(&mut self, played: &[f32]) {
		self.converted.clear();
		if self.resampler.process(played, &mut self.converted).is_err() || self.converted.is_empty() {
			return;
		}
		let mut timeline = TIMELINE.lock();
		let now = timeline.now();
		let start = self
			.cursor
			.filter(|cursor| cursor.abs_diff(now) <= WRITE_TOLERANCE)
			.unwrap_or(now);
		timeline.add(start, &self.converted);
		self.cursor = Some(start + self.converted.len() as u64);
	}
}

#[derive(Default)]
pub(crate) struct RenderReader {
	cursor: Option<u64>,
}

impl RenderReader {
	pub fn read(&mut self, backlog: usize, frame: &mut [f32]) {
		let timeline = TIMELINE.lock();
		let now = timeline.now();
		let length = frame.len() as u64;
		let captured = now.saturating_sub(backlog as u64);
		let start = self
			.cursor
			.filter(|start| start + length <= now && start + length + MAX_READ_LAG >= captured)
			.unwrap_or_else(|| captured.saturating_sub(length + READ_MARGIN));
		timeline.copy(start, frame);
		self.cursor = Some(start + length);
	}
}
