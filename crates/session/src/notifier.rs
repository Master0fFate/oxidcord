use std::time::Duration;

use state::Changes;
use tokio::time::Instant;

use crate::config::NotifyFn;

const BURST_WINDOW: Duration = Duration::from_millis(16);
const BURST_LIMIT: u32 = 60;

pub(crate) struct Notifier {
	notify: NotifyFn,
	deferred: Changes,
	window_start: Instant,
	window_events: u32,
}

impl Notifier {
	pub(crate) fn new(notify: NotifyFn) -> Self {
		Self {
			notify,
			deferred: Changes::default(),
			window_start: Instant::now(),
			window_events: 0,
		}
	}

	pub(crate) fn push(&mut self, changes: Changes) {
		let now = Instant::now();
		if now.duration_since(self.window_start) > BURST_WINDOW {
			self.window_start = now;
			self.window_events = 0;
		}
		self.window_events += 1;
		self.deferred.merge(changes);
		if self.window_events <= BURST_LIMIT {
			self.flush();
		}
	}

	pub(crate) fn flush(&mut self) {
		if !self.deferred.is_empty() {
			(self.notify)(std::mem::take(&mut self.deferred));
		}
	}

	pub(crate) fn deadline(&self) -> Option<Instant> {
		(!self.deferred.is_empty()).then(|| self.window_start + BURST_WINDOW)
	}
}
