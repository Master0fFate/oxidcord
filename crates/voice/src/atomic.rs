use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) struct AtomicF32(AtomicU32);

impl AtomicF32 {
	pub fn new(value: f32) -> Self {
		Self(AtomicU32::new(value.to_bits()))
	}

	pub fn load(&self) -> f32 {
		f32::from_bits(self.0.load(Ordering::Relaxed))
	}

	pub fn store(&self, value: f32) {
		self.0.store(value.to_bits(), Ordering::Relaxed);
	}
}
