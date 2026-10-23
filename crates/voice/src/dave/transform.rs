use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "testing")]
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;

use super::cryptor::{Decryptor, Encryptor, TRANSITION_EXPIRY};
use super::frame::{self, SILENCE};
use super::ratchet::KeyRatchet;
use crate::transform::{Codec, FrameTransform};

#[derive(Debug, Default)]
pub struct DaveStats {
	encrypted: AtomicU64,
	decrypted: AtomicU64,
	rejected: AtomicU64,
	enabled: AtomicBool,
}

impl DaveStats {
	pub fn encrypted(&self) -> u64 {
		self.encrypted.load(Ordering::Relaxed)
	}

	pub fn decrypted(&self) -> u64 {
		self.decrypted.load(Ordering::Relaxed)
	}

	pub fn rejected(&self) -> u64 {
		self.rejected.load(Ordering::Relaxed)
	}

	pub fn enabled(&self) -> bool {
		self.enabled.load(Ordering::Relaxed)
	}
}

#[derive(Default)]
struct Clock {
	#[cfg(feature = "testing")]
	skew: Duration,
}

impl Clock {
	fn now(&self) -> Instant {
		let now = Instant::now();
		#[cfg(feature = "testing")]
		let now = now + self.skew;
		now
	}
}

enum Sending {
	Plain,
	Blocked,
	Encrypt(Box<Encryptor>),
}

#[derive(Clone, Copy)]
enum Passthrough {
	Always,
	Until(Instant),
	Never,
}

impl Passthrough {
	fn allows(self, now: Instant) -> bool {
		match self {
			Self::Always => true,
			Self::Until(until) => until > now,
			Self::Never => false,
		}
	}

	fn limit(self, until: Instant) -> Self {
		match self {
			Self::Always => Self::Until(until),
			Self::Until(current) => Self::Until(current.min(until)),
			Self::Never => Self::Never,
		}
	}
}

pub struct FrameCrypto {
	clock: Clock,
	send: Sending,
	receive: Passthrough,
	decryptors: HashMap<u64, Decryptor>,
	stats: Arc<DaveStats>,
}

impl FrameCrypto {
	pub fn new(stats: Arc<DaveStats>) -> Self {
		Self {
			clock: Clock::default(),
			send: Sending::Plain,
			receive: Passthrough::Always,
			decryptors: HashMap::new(),
			stats,
		}
	}

	pub fn reset(&mut self, blocked: bool) {
		self.set_send(if blocked { Sending::Blocked } else { Sending::Plain });
		self.receive = Passthrough::Always;
		self.decryptors.clear();
	}

	pub fn install_senders(&mut self, ratchets: Vec<(u64, KeyRatchet)>) {
		let now = self.clock.now();
		self.decryptors
			.retain(|user, _| ratchets.iter().any(|(member, _)| member == user));
		for (user, ratchet) in ratchets {
			self.decryptors.entry(user).or_default().install(ratchet, now);
		}
	}

	pub fn start_sending(&mut self, ratchet: KeyRatchet) {
		let continuing = matches!(&self.send, Sending::Encrypt(encryptor) if encryptor.same_domain(&ratchet));
		if !continuing {
			self.set_send(Sending::Encrypt(Box::new(Encryptor::new(ratchet))));
		}
		self.receive = self.receive.limit(self.clock.now() + TRANSITION_EXPIRY);
	}

	pub fn block_sending(&mut self) {
		self.set_send(Sending::Blocked);
	}

	pub fn stop_sending(&mut self) {
		self.set_send(Sending::Plain);
		self.retire();
	}

	pub fn allow_plain_receive(&mut self) {
		self.receive = Passthrough::Always;
	}

	pub fn retire(&mut self) {
		let now = self.clock.now();
		for decryptor in self.decryptors.values_mut() {
			decryptor.retire(now);
		}
	}

	pub fn seal(&mut self, codec: Codec, frame: &[u8]) -> Option<Vec<u8>> {
		match &mut self.send {
			Sending::Plain => Some(frame.to_vec()),
			Sending::Blocked => None,
			Sending::Encrypt(encryptor) => {
				let sealed = encryptor.seal(codec, frame);
				if sealed.is_some() {
					self.stats.encrypted.fetch_add(1, Ordering::Relaxed);
				}
				sealed
			}
		}
	}

	pub fn open(&mut self, codec: Codec, sender: Option<u64>, payload: &[u8]) -> Option<Vec<u8>> {
		if codec == Codec::Opus && payload == SILENCE {
			return Some(payload.to_vec());
		}
		let now = self.clock.now();
		let opened = match frame::parse(payload) {
			None => self.receive.allows(now).then(|| payload.to_vec()),
			Some(parsed) => {
				let decrypted = sender
					.and_then(|user| self.decryptors.get_mut(&user))
					.and_then(|decryptor| decryptor.open(&parsed, now));
				if decrypted.is_some() {
					self.stats.decrypted.fetch_add(1, Ordering::Relaxed);
				}
				decrypted
			}
		};
		if opened.is_none() {
			self.stats.rejected.fetch_add(1, Ordering::Relaxed);
		}
		opened
	}

	fn set_send(&mut self, send: Sending) {
		self.stats
			.enabled
			.store(matches!(send, Sending::Encrypt(_)), Ordering::Relaxed);
		self.send = send;
	}

	#[cfg(feature = "testing")]
	pub fn advance_clock(&mut self, by: Duration) {
		self.clock.skew += by;
	}

	#[cfg(feature = "testing")]
	pub fn jump_generation(&mut self) -> Option<u32> {
		match &mut self.send {
			Sending::Encrypt(encryptor) => {
				encryptor.jump_to_generation_boundary();
				Some(encryptor.generation())
			}
			_ => None,
		}
	}

	#[cfg(feature = "testing")]
	pub fn generation(&self) -> Option<u32> {
		match &self.send {
			Sending::Encrypt(encryptor) => Some(encryptor.generation()),
			_ => None,
		}
	}
}

pub struct DaveTransform {
	crypto: Arc<Mutex<FrameCrypto>>,
	resolve: Box<dyn Fn(u32) -> Option<u64> + Send>,
}

impl DaveTransform {
	pub(super) fn new(
		crypto: Arc<Mutex<FrameCrypto>>,
		resolve: impl Fn(u32) -> Option<u64> + Send + 'static,
	) -> Self {
		Self {
			crypto,
			resolve: Box::new(resolve),
		}
	}

	#[cfg(feature = "testing")]
	pub fn advance_clock(&self, by: Duration) {
		self.crypto.lock().advance_clock(by);
	}

	#[cfg(feature = "testing")]
	pub fn jump_generation(&self) -> Option<u32> {
		self.crypto.lock().jump_generation()
	}

	#[cfg(feature = "testing")]
	pub fn generation(&self) -> Option<u32> {
		self.crypto.lock().generation()
	}
}

impl FrameTransform for DaveTransform {
	fn encrypt(&mut self, codec: Codec, frame: &[u8]) -> Option<Vec<u8>> {
		self.crypto.lock().seal(codec, frame)
	}

	fn decrypt(&mut self, codec: Codec, ssrc: u32, payload: &[u8]) -> Option<Vec<u8>> {
		let sender = (self.resolve)(ssrc);
		self.crypto.lock().open(codec, sender, payload)
	}
}
