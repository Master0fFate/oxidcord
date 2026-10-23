use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};

static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

pub fn set_reduced_motion(reduced: bool) {
	REDUCED_MOTION.store(reduced, Ordering::Relaxed);
}

pub struct Pairs<K, T> {
	entries: HashMap<K, (T, T)>,
}

impl<K, T> Default for Pairs<K, T> {
	fn default() -> Self {
		Self {
			entries: HashMap::new(),
		}
	}
}

impl<K: Eq + Hash, T: Copy + PartialEq> Pairs<K, T> {
	pub fn settle(&mut self, key: K, current: T) -> (T, T) {
		let entry = self.entries.entry(key).or_insert((current, current));
		if entry.1 != current {
			*entry = if REDUCED_MOTION.load(Ordering::Relaxed) {
				(current, current)
			} else {
				(entry.1, current)
			};
		}
		*entry
	}
}
