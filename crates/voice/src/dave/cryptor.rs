use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::codec;
use super::frame::{self, Cipher, GENERATION_SHIFT_BITS, Parsed};
use super::ratchet::KeyRatchet;
use crate::transform::Codec;

pub const TRANSITION_EXPIRY: Duration = Duration::from_secs(10);

const CRYPTOR_EXPIRY: Duration = Duration::from_secs(10);
const MAX_GENERATION_GAP: u32 = 250;
const MAX_MISSING_NONCES: usize = 1000;
const MAX_CIPHERTEXT_ATTEMPTS: usize = 10;
const MAX_FRAMES_PER_SECOND: u64 = 50 + 2 * 60;
const NONCE_MASK: u32 = (1 << GENERATION_SHIFT_BITS) - 1;

type BigNonce = u64;

struct ExpiringCipher {
	cipher: Cipher,
	expiry: Option<Instant>,
}

pub struct CryptorManager {
	ratchet: KeyRatchet,
	created: Instant,
	expiry: Option<Instant>,
	ciphers: HashMap<u32, ExpiringCipher>,
	oldest_generation: u32,
	newest_generation: u32,
	newest_nonce: Option<BigNonce>,
	missing_nonces: VecDeque<BigNonce>,
}

impl CryptorManager {
	pub fn new(ratchet: KeyRatchet, now: Instant) -> Self {
		Self {
			ratchet,
			created: now,
			expiry: None,
			ciphers: HashMap::new(),
			oldest_generation: 0,
			newest_generation: 0,
			newest_nonce: None,
			missing_nonces: VecDeque::new(),
		}
	}

	pub fn same_domain(&self, ratchet: &KeyRatchet) -> bool {
		self.ratchet.same_domain(ratchet)
	}

	pub fn limit_expiry(&mut self, expiry: Instant) {
		self.expiry = Some(self.expiry.map_or(expiry, |current| current.min(expiry)));
	}

	pub fn is_expired(&self, now: Instant) -> bool {
		self.expiry.is_some_and(|expiry| now > expiry)
	}

	pub fn open(&mut self, parsed: &Parsed, now: Instant) -> Option<Vec<u8>> {
		let generation =
			frame::wrapped_generation(self.oldest_generation, parsed.nonce >> GENERATION_SHIFT_BITS);
		if !self.can_process(generation, parsed.nonce) {
			return None;
		}
		let cipher = self.cipher(generation, now)?;
		let plain = parsed.open(cipher)?;
		self.report_success(generation, parsed.nonce, now);
		Some(plain)
	}

	fn big_nonce(generation: u32, nonce: u32) -> BigNonce {
		(BigNonce::from(generation) << GENERATION_SHIFT_BITS) | BigNonce::from(nonce & NONCE_MASK)
	}

	fn expected_nonce(&self) -> BigNonce {
		self.newest_nonce.map_or(0, |newest| newest + 1)
	}

	fn can_process(&self, generation: u32, nonce: u32) -> bool {
		let big = Self::big_nonce(generation, nonce);
		big >= self.expected_nonce() || self.missing_nonces.iter().rev().any(|missing| *missing == big)
	}

	fn cipher(&mut self, generation: u32, now: Instant) -> Option<&Cipher> {
		self.drop_expired(now);
		if generation < self.oldest_generation || generation > self.newest_generation + MAX_GENERATION_GAP {
			return None;
		}
		let lifetime_seconds = now.saturating_duration_since(self.created).as_secs();
		let max_generation = (MAX_FRAMES_PER_SECOND * lifetime_seconds) >> GENERATION_SHIFT_BITS;
		if u64::from(generation) > max_generation {
			return None;
		}
		if !self.ciphers.contains_key(&generation) {
			let key = self.ratchet.key(generation)?;
			let expiry = (generation < self.newest_generation).then(|| now + CRYPTOR_EXPIRY);
			self.ciphers.insert(
				generation,
				ExpiringCipher {
					cipher: Cipher::new(&key),
					expiry,
				},
			);
		}
		self.ciphers.get(&generation).map(|entry| &entry.cipher)
	}

	fn report_success(&mut self, generation: u32, nonce: u32, now: Instant) {
		let big = Self::big_nonce(generation, nonce);
		let expected = self.expected_nonce();
		if big >= expected {
			let gap =
				usize::try_from(big - expected).map_or(MAX_MISSING_NONCES, |gap| gap.min(MAX_MISSING_NONCES));
			while !self.missing_nonces.is_empty() && self.missing_nonces.len() + gap > MAX_MISSING_NONCES {
				self.missing_nonces.pop_front();
			}
			self.missing_nonces.extend(big - gap as u64..big);
			self.newest_nonce = Some(big);
		} else if let Some(position) = self.missing_nonces.iter().rposition(|missing| *missing == big) {
			self.missing_nonces.remove(position);
		}
		if generation <= self.newest_generation || !self.ciphers.contains_key(&generation) {
			return;
		}
		self.newest_generation = generation;
		let expiry = now + CRYPTOR_EXPIRY;
		for (older, entry) in &mut self.ciphers {
			if *older < generation {
				entry.expiry = Some(entry.expiry.map_or(expiry, |current| current.min(expiry)));
			}
		}
	}

	fn drop_expired(&mut self, now: Instant) {
		self.ciphers
			.retain(|_, entry| !entry.expiry.is_some_and(|expiry| expiry < now));
		while self.oldest_generation < self.newest_generation
			&& !self.ciphers.contains_key(&self.oldest_generation)
		{
			self.ratchet.erase(self.oldest_generation);
			self.oldest_generation += 1;
		}
	}
}

#[derive(Default)]
pub struct Decryptor {
	managers: VecDeque<CryptorManager>,
}

impl Decryptor {
	pub fn install(&mut self, ratchet: KeyRatchet, now: Instant) {
		self.drop_expired(now);
		if self.managers.iter().any(|manager| manager.same_domain(&ratchet)) {
			return;
		}
		self.retire(now);
		self.managers.push_back(CryptorManager::new(ratchet, now));
	}

	pub fn retire(&mut self, now: Instant) {
		let expiry = now + TRANSITION_EXPIRY;
		for manager in &mut self.managers {
			manager.limit_expiry(expiry);
		}
	}

	pub fn open(&mut self, parsed: &Parsed, now: Instant) -> Option<Vec<u8>> {
		self.drop_expired(now);
		self.managers
			.iter_mut()
			.rev()
			.find_map(|manager| manager.open(parsed, now))
	}

	fn drop_expired(&mut self, now: Instant) {
		while self
			.managers
			.front()
			.is_some_and(|manager| manager.is_expired(now))
		{
			self.managers.pop_front();
		}
	}
}

pub struct Encryptor {
	ratchet: KeyRatchet,
	cipher: Option<Cipher>,
	generation: u32,
	nonce: u32,
}

impl Encryptor {
	pub fn new(ratchet: KeyRatchet) -> Self {
		Self {
			ratchet,
			cipher: None,
			generation: 0,
			nonce: 0,
		}
	}

	pub fn same_domain(&self, ratchet: &KeyRatchet) -> bool {
		self.ratchet.same_domain(ratchet)
	}

	pub fn seal(&mut self, codec: Codec, media: &[u8]) -> Option<Vec<u8>> {
		match codec {
			Codec::Opus => self.seal_once(media, &[]),
			Codec::H264 => {
				let processed = codec::process_h264(media).unwrap_or_else(|| codec::Processed {
					frame: media.to_vec(),
					ranges: Vec::new(),
				});
				(0..MAX_CIPHERTEXT_ATTEMPTS).find_map(|_| {
					let sealed = self.seal_once(&processed.frame, &processed.ranges)?;
					codec::ciphertext_is_packetizable(&sealed, &processed.ranges).then_some(sealed)
				})
			}
		}
	}

	fn seal_once(&mut self, media: &[u8], ranges: &[(usize, usize)]) -> Option<Vec<u8>> {
		self.nonce = self.nonce.wrapping_add(1);
		let generation = frame::wrapped_generation(self.generation, self.nonce >> GENERATION_SHIFT_BITS);
		if generation != self.generation || self.cipher.is_none() {
			let key = self.ratchet.key(generation)?;
			if generation != self.generation {
				self.ratchet.erase(self.generation);
			}
			self.generation = generation;
			self.cipher = Some(Cipher::new(&key));
		}
		frame::seal(self.cipher.as_ref()?, self.nonce, media, ranges)
	}

	#[cfg(feature = "testing")]
	pub fn jump_to_generation_boundary(&mut self) {
		let next = (self.nonce >> GENERATION_SHIFT_BITS).wrapping_add(1) << GENERATION_SHIFT_BITS;
		self.nonce = next.wrapping_sub(1);
	}

	#[cfg(feature = "testing")]
	pub fn generation(&self) -> u32 {
		self.generation
	}
}
