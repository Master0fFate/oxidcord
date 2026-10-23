use std::collections::HashMap;

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const KEY_BYTES: usize = 16;
const SECRET_BYTES: usize = 32;
const LABEL_PREFIX: &[u8] = b"MLS 1.0 ";
const GENERATION_CONTEXT_BYTES: u8 = 4;
const FIRST_BLOCK_COUNTER: u8 = 1;

pub type Key = [u8; KEY_BYTES];

pub struct KeyRatchet {
	base: Key,
	next_secret: [u8; SECRET_BYTES],
	secret_len: usize,
	next_generation: u32,
	keys: HashMap<u32, Key>,
}

impl KeyRatchet {
	pub fn new(base: Key) -> Self {
		let mut next_secret = [0; SECRET_BYTES];
		next_secret[..KEY_BYTES].copy_from_slice(&base);
		Self {
			base,
			next_secret,
			secret_len: KEY_BYTES,
			next_generation: 0,
			keys: HashMap::new(),
		}
	}

	pub fn same_domain(&self, other: &Self) -> bool {
		self.base == other.base
	}

	pub fn key(&mut self, generation: u32) -> Option<Key> {
		if let Some(key) = self.keys.get(&generation) {
			return Some(*key);
		}
		if self.next_generation > generation {
			return None;
		}
		while self.next_generation <= generation {
			self.advance();
		}
		self.keys.get(&generation).copied()
	}

	pub fn erase(&mut self, generation: u32) {
		self.keys.remove(&generation);
	}

	fn advance(&mut self) {
		let generation = self.next_generation;
		let secret = &self.next_secret[..self.secret_len];
		let mut key = [0; KEY_BYTES];
		key.copy_from_slice(&derive(secret, b"key", generation)[..KEY_BYTES]);
		self.next_secret = derive(secret, b"secret", generation);
		self.secret_len = SECRET_BYTES;
		self.next_generation = generation.wrapping_add(1);
		self.keys.insert(generation, key);
	}
}

fn derive(secret: &[u8], label: &[u8], generation: u32) -> [u8; SECRET_BYTES] {
	let length = if label == b"key" { KEY_BYTES } else { SECRET_BYTES };
	let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
	mac.update(&(length as u16).to_be_bytes());
	mac.update(&[(LABEL_PREFIX.len() + label.len()) as u8]);
	mac.update(LABEL_PREFIX);
	mac.update(label);
	mac.update(&[GENERATION_CONTEXT_BYTES]);
	mac.update(&generation.to_be_bytes());
	mac.update(&[FIRST_BLOCK_COUNTER]);
	mac.finalize().into_bytes().into()
}
