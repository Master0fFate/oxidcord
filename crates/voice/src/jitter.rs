use std::collections::BTreeMap;
use std::time::Instant;

pub const MIN_DEPTH: usize = 3;
pub const MAX_DEPTH: usize = 10;

const FRAME_MS: f64 = 20.0;
const TIMESTAMP_TICKS_PER_MS: f64 = 48.0;
const MAX_QUEUED: usize = 32;
const OVERFLOW_SLACK: usize = 4;
const PREFILL_TIMEOUT_FRAMES: u32 = 3;
const IDLE_RESET_FRAMES: u32 = 10;
const CLEAN_FRAMES_PER_RELAX: u32 = 250;
const MAX_PENALTY: usize = 7;
const MAX_TRANSIT_JUMP_MS: f64 = 500.0;
const SEQUENCE_BASE: i64 = 1 << 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playout {
	Silence,
	Packet(Vec<u8>),
	Missing { recovery: Option<Vec<u8>> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
	Queued,
	Late,
	Duplicate,
}

pub struct JitterBuffer {
	queue: BTreeMap<i64, Vec<u8>>,
	newest: Option<i64>,
	next: Option<i64>,
	playing: bool,
	arrived: bool,
	idle_frames: u32,
	penalty: usize,
	clean_frames: u32,
	jitter_ms: f64,
	transit_ms: Option<f64>,
	epoch: Instant,
}

impl JitterBuffer {
	pub fn new(now: Instant) -> Self {
		Self {
			queue: BTreeMap::new(),
			newest: None,
			next: None,
			playing: false,
			arrived: false,
			idle_frames: 0,
			penalty: 0,
			clean_frames: 0,
			jitter_ms: 0.0,
			transit_ms: None,
			epoch: now,
		}
	}

	pub fn push(&mut self, sequence: u16, timestamp: u32, data: Vec<u8>, now: Instant) -> Push {
		let extended = self.extend(sequence);
		self.track_jitter(timestamp, now);
		if self.playing && self.next.is_some_and(|next| extended < next) {
			self.penalize();
			return Push::Late;
		}
		if self.queue.contains_key(&extended) {
			return Push::Duplicate;
		}
		if self.playing
			&& self.queue.is_empty()
			&& self.next == Some(extended)
			&& (1..IDLE_RESET_FRAMES).contains(&self.idle_frames)
		{
			self.penalize();
		}
		self.queue.insert(extended, data);
		self.arrived = true;
		self.newest = Some(self.newest.map_or(extended, |newest| newest.max(extended)));
		if self.queue.len() > MAX_QUEUED {
			self.queue.pop_first();
			if self.playing {
				self.next = self.queue.keys().next().copied();
			}
		}
		Push::Queued
	}

	pub fn pop(&mut self) -> Playout {
		if std::mem::take(&mut self.arrived) {
			self.idle_frames = 0;
		} else {
			self.idle_frames = self.idle_frames.saturating_add(1);
		}
		if !self.playing {
			let ready = self.queue.len() >= self.target_depth()
				|| (!self.queue.is_empty() && self.idle_frames >= PREFILL_TIMEOUT_FRAMES);
			if !ready {
				self.reset_if_idle();
				return Playout::Silence;
			}
			self.playing = true;
			self.next = self.queue.keys().next().copied();
		}
		self.drop_overflow();
		let Some(mut next) = self.next else {
			return Playout::Silence;
		};
		if let Some(&first) = self.queue.keys().next()
			&& first - next > MAX_QUEUED as i64
		{
			next = first;
		}
		if let Some(data) = self.queue.remove(&next) {
			self.next = Some(next + 1);
			self.relax();
			return Playout::Packet(data);
		}
		if self.queue.is_empty() {
			self.reset_if_idle();
			return Playout::Silence;
		}
		self.next = Some(next + 1);
		Playout::Missing {
			recovery: self.queue.get(&(next + 1)).cloned(),
		}
	}

	pub fn jitter_ms(&self) -> f32 {
		self.jitter_ms as f32
	}

	pub fn target_depth(&self) -> usize {
		let jitter_frames = (self.jitter_ms * 2.0 / FRAME_MS).ceil() as usize;
		(MIN_DEPTH + self.penalty + jitter_frames).clamp(MIN_DEPTH, MAX_DEPTH)
	}

	pub fn is_idle(&self) -> bool {
		!self.playing && self.queue.is_empty()
	}

	fn extend(&self, sequence: u16) -> i64 {
		match self.newest {
			None => SEQUENCE_BASE + i64::from(sequence),
			Some(newest) => newest + i64::from(sequence.wrapping_sub(newest as u16) as i16),
		}
	}

	fn track_jitter(&mut self, timestamp: u32, now: Instant) {
		let arrival_ms = now.duration_since(self.epoch).as_secs_f64() * 1000.0;
		let transit_ms = arrival_ms - f64::from(timestamp) / TIMESTAMP_TICKS_PER_MS;
		if let Some(previous) = self.transit_ms {
			let deviation = (transit_ms - previous).abs();
			if deviation < MAX_TRANSIT_JUMP_MS {
				self.jitter_ms += (deviation - self.jitter_ms) / 16.0;
			}
		}
		self.transit_ms = Some(transit_ms);
	}

	fn drop_overflow(&mut self) {
		if self.queue.len() > self.target_depth() + OVERFLOW_SLACK {
			self.queue.pop_first();
			self.next = self.queue.keys().next().copied();
		}
	}

	fn penalize(&mut self) {
		self.penalty = (self.penalty + 1).min(MAX_PENALTY);
		self.clean_frames = 0;
	}

	fn relax(&mut self) {
		self.clean_frames += 1;
		if self.clean_frames >= CLEAN_FRAMES_PER_RELAX {
			self.clean_frames = 0;
			self.penalty = self.penalty.saturating_sub(1);
		}
	}

	fn reset_if_idle(&mut self) {
		if self.idle_frames < IDLE_RESET_FRAMES {
			return;
		}
		self.queue.clear();
		self.newest = None;
		self.next = None;
		self.playing = false;
		self.transit_ms = None;
	}
}
