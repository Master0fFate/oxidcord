use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Bucket {
	capacity: f64,
	tokens: f64,
	per_second: f64,
	updated: Instant,
}

impl Bucket {
	pub fn new(burst: u32, refill_every: Duration) -> Self {
		Self {
			capacity: f64::from(burst),
			tokens: f64::from(burst),
			per_second: 1.0 / refill_every.as_secs_f64(),
			updated: Instant::now(),
		}
	}

	pub fn take(&mut self) -> bool {
		let now = Instant::now();
		let elapsed = now.duration_since(self.updated).as_secs_f64();
		self.updated = now;
		self.tokens = (self.tokens + elapsed * self.per_second).min(self.capacity);
		if self.tokens >= 1.0 {
			self.tokens -= 1.0;
			true
		} else {
			false
		}
	}
}

#[derive(Debug)]
pub struct RateLimits {
	pub log: Bucket,
	pub send_message: Bucket,
	pub add_reaction: Bucket,
	pub notify: Bucket,
	pub fetch: Bucket,
	pub open_url: Bucket,
}

impl RateLimits {
	pub fn new() -> Self {
		Self {
			log: Bucket::new(200, Duration::from_millis(10)),
			send_message: Bucket::new(3, Duration::from_secs(2)),
			add_reaction: Bucket::new(5, Duration::from_secs(1)),
			notify: Bucket::new(1, Duration::from_secs(5)),
			fetch: Bucket::new(20, Duration::from_secs(6)),
			open_url: Bucket::new(1, Duration::from_secs(10)),
		}
	}
}

impl Default for RateLimits {
	fn default() -> Self {
		Self::new()
	}
}
