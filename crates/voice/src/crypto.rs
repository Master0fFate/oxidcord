use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{AeadCore, AeadInOut, KeyInit, Nonce};
use chacha20poly1305::XChaCha20Poly1305;

use crate::error::{Result, VoiceError};

const COUNTER_LEN: usize = 4;
const TAG_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMode {
	AeadAes256GcmRtpSize,
	AeadXChaCha20Poly1305RtpSize,
}

impl EncryptionMode {
	const PREFERENCE: [Self; 2] = [Self::AeadAes256GcmRtpSize, Self::AeadXChaCha20Poly1305RtpSize];

	pub fn name(self) -> &'static str {
		match self {
			Self::AeadAes256GcmRtpSize => "aead_aes256_gcm_rtpsize",
			Self::AeadXChaCha20Poly1305RtpSize => "aead_xchacha20_poly1305_rtpsize",
		}
	}

	pub fn from_name(name: &str) -> Option<Self> {
		Self::PREFERENCE.into_iter().find(|mode| mode.name() == name)
	}

	pub fn select(offered: &[String]) -> Option<Self> {
		Self::PREFERENCE
			.into_iter()
			.find(|mode| offered.iter().any(|name| name == mode.name()))
	}
}

enum Aead {
	Aes(Box<Aes256Gcm>),
	XChaCha(Box<XChaCha20Poly1305>),
}

pub(crate) struct PacketCipher {
	aead: Aead,
	counter: u32,
}

impl PacketCipher {
	pub fn new(mode: EncryptionMode, key: &[u8]) -> Result<Self> {
		let aead = match mode {
			EncryptionMode::AeadAes256GcmRtpSize => Aead::Aes(Box::new(
				Aes256Gcm::new_from_slice(key).map_err(|_| VoiceError::InvalidKey)?,
			)),
			EncryptionMode::AeadXChaCha20Poly1305RtpSize => Aead::XChaCha(Box::new(
				XChaCha20Poly1305::new_from_slice(key).map_err(|_| VoiceError::InvalidKey)?,
			)),
		};
		Ok(Self { aead, counter: 0 })
	}

	pub fn seal(&mut self, header: &[u8], payload: &[u8]) -> Option<Vec<u8>> {
		let counter = self.counter.to_be_bytes();
		self.counter = self.counter.wrapping_add(1);
		let mut body = Vec::with_capacity(payload.len() + TAG_LEN);
		body.extend_from_slice(payload);
		self.aead.encrypt(&counter, header, &mut body)?;
		let mut packet = Vec::with_capacity(header.len() + body.len() + COUNTER_LEN);
		packet.extend_from_slice(header);
		packet.extend_from_slice(&body);
		packet.extend_from_slice(&counter);
		Some(packet)
	}

	pub fn open(&self, packet: &[u8], header_len: usize) -> Option<Vec<u8>> {
		if packet.len() < header_len + TAG_LEN + COUNTER_LEN {
			return None;
		}
		let (sealed, counter) = packet.split_at(packet.len() - COUNTER_LEN);
		let (header, body) = sealed.split_at(header_len);
		let mut plain = body.to_vec();
		self.aead.decrypt(counter, header, &mut plain)?;
		Some(plain)
	}
}

impl Aead {
	fn encrypt(&self, counter: &[u8], header: &[u8], body: &mut Vec<u8>) -> Option<()> {
		match self {
			Self::Aes(cipher) => cipher.encrypt_in_place(&padded::<Aes256Gcm>(counter), header, body),
			Self::XChaCha(cipher) => {
				cipher.encrypt_in_place(&padded::<XChaCha20Poly1305>(counter), header, body)
			}
		}
		.ok()
	}

	fn decrypt(&self, counter: &[u8], header: &[u8], body: &mut Vec<u8>) -> Option<()> {
		match self {
			Self::Aes(cipher) => cipher.decrypt_in_place(&padded::<Aes256Gcm>(counter), header, body),
			Self::XChaCha(cipher) => {
				cipher.decrypt_in_place(&padded::<XChaCha20Poly1305>(counter), header, body)
			}
		}
		.ok()
	}
}

fn padded<A: AeadCore>(counter: &[u8]) -> Nonce<A> {
	let mut nonce = Nonce::<A>::default();
	nonce[..COUNTER_LEN].copy_from_slice(counter);
	nonce
}
