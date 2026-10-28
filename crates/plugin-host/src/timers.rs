use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::instance::PluginInstance;

pub const MIN_DELAY: Duration = Duration::from_millis(250);
pub const MIN_REPEAT: Duration = Duration::from_secs(1);
pub const MAX_TIMERS_PER_PLUGIN: usize = 64;

struct Timer {
	plugin_id: Arc<str>,
	tag: String,
	due: Instant,
	repeat: Option<Duration>,
}

#[derive(Default)]
struct State {
	heap: BinaryHeap<Reverse<(Instant, u64)>>,
	timers: HashMap<u64, Timer>,
	owned: HashMap<Arc<str>, usize>,
	targets: HashMap<Arc<str>, Weak<PluginInstance>>,
	next_id: u64,
	closed: bool,
}

#[derive(Default)]
struct Shared {
	state: Mutex<State>,
	wake: Condvar,
}

impl State {
	fn remove(&mut self, id: u64) {
		if let Some(timer) = self.timers.remove(&id) {
			self.release(&timer.plugin_id, 1);
		}
	}

	fn release(&mut self, plugin_id: &str, count: usize) {
		if let Some(owned) = self.owned.get_mut(plugin_id) {
			*owned = owned.saturating_sub(count);
			if *owned == 0 {
				self.owned.remove(plugin_id);
			}
		}
	}

	fn remove_all(&mut self, plugin_id: &str) {
		self.timers.retain(|_, timer| &*timer.plugin_id != plugin_id);
		self.owned.remove(plugin_id);
	}
}

pub enum TimerError {
	Limit,
}

pub struct Timers {
	shared: Arc<Shared>,
}

impl Timers {
	pub fn spawn() -> std::io::Result<Self> {
		let shared = Arc::new(Shared::default());
		let worker = Arc::clone(&shared);
		thread::Builder::new()
			.name("plugin-timers".to_owned())
			.spawn(move || run(&worker))?;
		Ok(Self { shared })
	}

	pub fn attach(&self, plugin_id: &Arc<str>, instance: Weak<PluginInstance>) {
		self.shared
			.state
			.lock()
			.targets
			.insert(Arc::clone(plugin_id), instance);
	}

	pub fn set(
		&self,
		plugin_id: &Arc<str>,
		delay: Duration,
		repeat: Option<Duration>,
		tag: String,
	) -> Result<u64, TimerError> {
		let mut state = self.shared.state.lock();
		let owned = state.owned.entry(Arc::clone(plugin_id)).or_default();
		if *owned >= MAX_TIMERS_PER_PLUGIN {
			return Err(TimerError::Limit);
		}
		*owned += 1;
		state.next_id += 1;
		let id = state.next_id;
		let due = Instant::now() + delay;
		state.timers.insert(
			id,
			Timer {
				plugin_id: Arc::clone(plugin_id),
				tag,
				due,
				repeat,
			},
		);
		let earliest = state.heap.peek().is_none_or(|Reverse((next, _))| due < *next);
		state.heap.push(Reverse((due, id)));
		drop(state);
		if earliest {
			self.shared.wake.notify_one();
		}
		Ok(id)
	}

	pub fn clear(&self, plugin_id: &str, id: u64) {
		let mut state = self.shared.state.lock();
		if state
			.timers
			.get(&id)
			.is_some_and(|timer| &*timer.plugin_id == plugin_id)
		{
			state.remove(id);
		}
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		self.shared.state.lock().remove_all(plugin_id);
	}

	pub fn detach(&self, plugin_id: &str) {
		let mut state = self.shared.state.lock();
		state.targets.remove(plugin_id);
		state.remove_all(plugin_id);
	}
}

impl Drop for Timers {
	fn drop(&mut self) {
		self.shared.state.lock().closed = true;
		self.shared.wake.notify_all();
	}
}

fn run(shared: &Shared) {
	let mut state = shared.state.lock();
	loop {
		if state.closed {
			return;
		}
		let Some(&Reverse((due, id))) = state.heap.peek() else {
			shared.wake.wait(&mut state);
			continue;
		};
		let now = Instant::now();
		if due > now {
			shared.wake.wait_until(&mut state, due);
			continue;
		}
		state.heap.pop();
		let Some(timer) = state.timers.get_mut(&id).filter(|timer| timer.due == due) else {
			continue;
		};
		let plugin_id = Arc::clone(&timer.plugin_id);
		let tag = timer.tag.clone();
		match timer.repeat {
			Some(repeat) => {
				let mut next = due + repeat;
				if next <= now {
					next = now + repeat;
				}
				timer.due = next;
				state.heap.push(Reverse((next, id)));
			}
			None => state.remove(id),
		}
		let target = state.targets.get(&plugin_id).and_then(Weak::upgrade);
		drop(state);
		if let Some(instance) = target {
			instance.post_timer(id, tag);
		}
		state = shared.state.lock();
	}
}
