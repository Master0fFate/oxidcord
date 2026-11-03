use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::{Instant, sleep_until};

pub(crate) struct SendWindow {
	permits: usize,
	window: Duration,
	sent: VecDeque<Instant>,
}

impl SendWindow {
	pub fn new(permits: usize, window: Duration) -> Self {
		Self {
			permits,
			window,
			sent: VecDeque::with_capacity(permits),
		}
	}

	pub async fn room(&mut self, reserved: usize) {
		let limit = self.permits.saturating_sub(reserved).max(1);
		loop {
			self.expire();
			if self.sent.len() < limit {
				return;
			}
			sleep_until(self.sent[self.sent.len() - limit] + self.window).await;
		}
	}

	pub fn record(&mut self) {
		self.expire();
		self.sent.push_back(Instant::now());
	}

	fn expire(&mut self) {
		let now = Instant::now();
		while self
			.sent
			.front()
			.is_some_and(|sent| now.duration_since(*sent) >= self.window)
		{
			self.sent.pop_front();
		}
	}
}
