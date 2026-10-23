use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use state::LocalEvent;

use super::{Core, Internal};

#[derive(Default)]
pub(super) struct MicTestRun {
	generation: u64,
	cancel: Arc<AtomicBool>,
	running: bool,
}

impl Core {
	pub(super) fn start_mic_test(&mut self, seconds: u32) {
		if self.mic_test.running {
			if self.mic_test.cancel.load(Ordering::Relaxed) {
				self.reject("The previous mic test is still stopping");
			}
			return;
		}
		self.mic_test.generation += 1;
		self.mic_test.running = true;
		self.mic_test.cancel = Arc::new(AtomicBool::new(false));
		let generation = self.mic_test.generation;
		let cancel = self.mic_test.cancel.clone();
		let internal = self.internal.clone();
		self.apply_local(LocalEvent::MicTestStarted { seconds });
		self.runtime.spawn_blocking(move || {
			let started = Instant::now();
			let report = |level: f32| {
				if cancel.load(Ordering::Relaxed) {
					return;
				}
				let elapsed = u32::try_from(started.elapsed().as_secs()).unwrap_or(u32::MAX);
				let _ = internal.send(Internal::MicTestLevel {
					generation,
					level,
					remaining_secs: seconds.saturating_sub(elapsed),
				});
			};
			let result = ::voice::diagnostics::loopback_with_levels(seconds, &report);
			let _ = internal.send(Internal::MicTestFinished {
				generation,
				result: result.map_err(|error| error.to_string()),
			});
		});
	}

	pub(super) fn stop_mic_test(&mut self) {
		if self.mic_test.running {
			self.mic_test.cancel.store(true, Ordering::Relaxed);
		}
		self.apply_local(LocalEvent::MicTestFinished);
	}

	pub(super) fn on_mic_test_level(&mut self, generation: u64, level: f32, remaining_secs: u32) {
		if generation == self.mic_test.generation && !self.mic_test.cancel.load(Ordering::Relaxed) {
			self.apply_local(LocalEvent::MicTestLevel {
				level,
				remaining_secs,
			});
		}
	}

	pub(super) fn on_mic_test_finished(&mut self, generation: u64, result: Result<(), String>) {
		if generation != self.mic_test.generation {
			return;
		}
		self.mic_test.running = false;
		if self.mic_test.cancel.load(Ordering::Relaxed) {
			return;
		}
		self.apply_local(LocalEvent::MicTestFinished);
		if let Err(reason) = result {
			tracing::warn!(reason, "mic test failed");
			self.reject(format!("Mic test failed: {reason}"));
		}
	}
}
