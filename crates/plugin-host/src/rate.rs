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
	pub ui: Bucket,
	pub toast: Bucket,
	pub timer: Bucket,
	pub composer: Bucket,
	pub lookup: Bucket,
	pub fetch_message: Bucket,
	pub reaction_users: Bucket,
	pub attachment_bytes: Bucket,
	pub speak: Bucket,
	pub presence: Bucket,
	pub ack: Bucket,
	pub settings: Bucket,
	pub invalidate: Bucket,
	pub action: Bucket,
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
			ui: Bucket::new(60, Duration::from_millis(50)),
			toast: Bucket::new(1, Duration::from_secs(5)),
			timer: Bucket::new(32, Duration::from_millis(250)),
			composer: Bucket::new(5, Duration::from_secs(1)),
			lookup: Bucket::new(5, Duration::from_secs(2)),
			fetch_message: Bucket::new(5, Duration::from_secs(2)),
			reaction_users: Bucket::new(5, Duration::from_secs(2)),
			attachment_bytes: Bucket::new(10, Duration::from_secs(6)),
			speak: Bucket::new(1, Duration::from_secs(2)),
			presence: Bucket::new(2, Duration::from_secs(5)),
			ack: Bucket::new(5, Duration::from_secs(1)),
			settings: Bucket::new(5, Duration::from_secs(2)),
			invalidate: Bucket::new(50, Duration::from_millis(100)),
			action: Bucket::new(5, Duration::from_secs(1)),
		}
	}
}

impl Default for RateLimits {
	fn default() -> Self {
		Self::new()
	}
}
