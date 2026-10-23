use std::time::Duration;

use tokio::time::{Instant, sleep};

pub(crate) struct RateLimiter {
	capacity: f64,
	per_second: f64,
	tokens: f64,
	updated: Instant,
}

impl RateLimiter {
	pub fn new(permits: u32, window: Duration) -> Self {
		let capacity = f64::from(permits);
		Self {
			capacity,
			per_second: capacity / window.as_secs_f64(),
			tokens: capacity,
			updated: Instant::now(),
		}
	}

	pub async fn ready(&mut self) {
		loop {
			self.refill();
			if self.tokens >= 1.0 {
				return;
			}
			sleep(Duration::from_secs_f64((1.0 - self.tokens) / self.per_second)).await;
		}
	}

	pub fn take(&mut self) {
		self.refill();
		self.tokens -= 1.0;
	}

	fn refill(&mut self) {
		let now = Instant::now();
		let earned = now.duration_since(self.updated).as_secs_f64() * self.per_second;
		self.tokens = (self.tokens + earned).min(self.capacity);
		self.updated = now;
	}
}
