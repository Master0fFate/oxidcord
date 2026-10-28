use std::time::Instant;

use parking_lot::Mutex;

pub(crate) struct ClientMarks {
	launched: Instant,
	phases: Mutex<Vec<(&'static str, u64)>>,
}

impl ClientMarks {
	pub(crate) fn new() -> Self {
		Self {
			launched: Instant::now(),
			phases: Mutex::new(vec![("launch", 0)]),
		}
	}

	pub(crate) fn record(&self, phase: &'static str) {
		let mut phases = self.phases.lock();
		if phases.iter().all(|(known, _)| *known != phase) {
			phases.push((phase, self.uptime_ms()));
		}
	}

	pub(crate) fn uptime_ms(&self) -> u64 {
		u64::try_from(self.launched.elapsed().as_millis()).unwrap_or(u64::MAX)
	}

	pub(crate) fn startup(&self) -> Vec<(String, i64)> {
		self.phases
			.lock()
			.iter()
			.map(|(phase, millis)| ((*phase).to_owned(), i64::try_from(*millis).unwrap_or(i64::MAX)))
			.collect()
	}
}
