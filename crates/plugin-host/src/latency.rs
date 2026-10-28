use std::collections::VecDeque;
use std::time::Duration;

use parking_lot::Mutex;

const SAMPLE_CAPACITY: usize = 512;
const P95: f64 = 0.95;

#[derive(Default)]
pub struct LatencyStats {
	samples: Mutex<VecDeque<f32>>,
}

impl LatencyStats {
	pub fn record(&self, elapsed: Duration) {
		let mut samples = self.samples.lock();
		if samples.len() == SAMPLE_CAPACITY {
			samples.pop_front();
		}
		samples.push_back(elapsed.as_secs_f32() * 1000.0);
	}

	pub fn p95_ms(&self) -> Option<f64> {
		let mut sorted: Vec<f32> = self.samples.lock().iter().copied().collect();
		if sorted.is_empty() {
			return None;
		}
		sorted.sort_by(f32::total_cmp);
		let index = ((sorted.len() as f64 * P95).ceil() as usize).clamp(1, sorted.len()) - 1;
		Some(f64::from(sorted[index]))
	}
}
