use aes_gcm::aead::consts::{U8, U12};
use aes_gcm::aead::inout::InOutBuf;
use aes_gcm::aes::Aes128;
use aes_gcm::{AeadInOut, AesGcm, KeyInit};

use super::ratchet::Key;

pub const TAG_BYTES: usize = 8;
pub const NONCE_BYTES: usize = 12;
pub const SYNC_NONCE_OFFSET: usize = 8;
pub const GENERATION_SHIFT_BITS: u32 = 24;
pub const GENERATION_WRAP: u32 = 1 << (u32::BITS - GENERATION_SHIFT_BITS);
pub const MARKER: [u8; 2] = [0xfa, 0xfa];
pub const SILENCE: [u8; 3] = [0xf8, 0xff, 0xfe];

const SIZE_BYTES: usize = 1;
const MIN_SUPPLEMENTAL_BYTES: usize = TAG_BYTES + SIZE_BYTES + MARKER.len();
const MAX_LEB128_BYTES: usize = 10;
const LEB128_PAYLOAD_BITS: u32 = 7;
const LEB128_CONTINUE: u8 = 0x80;
const LEB128_PAYLOAD_MASK: u8 = 0x7f;

pub type Tag = [u8; TAG_BYTES];
pub type Nonce = [u8; NONCE_BYTES];

pub struct Cipher(AesGcm<Aes128, U12, U8>);

impl Cipher {
	pub fn new(key: &Key) -> Self {
		Self(AesGcm::new(key.into()))
	}

	pub fn seal(&self, nonce: u32, plaintext: &mut [u8], associated: &[u8]) -> Option<Tag> {
		let tag = self
			.0
			.encrypt_inout_detached(&expand(nonce).into(), associated, InOutBuf::from(plaintext))
			.ok()?;
		Some(tag.into())
	}

	pub fn open(&self, nonce: u32, ciphertext: &mut [u8], associated: &[u8], tag: &Tag) -> Option<()> {
		self.0
			.decrypt_inout_detached(
				&expand(nonce).into(),
				associated,
				InOutBuf::from(ciphertext),
				&(*tag).into(),
			)
			.ok()
	}
}

fn expand(nonce: u32) -> Nonce {
	let mut full = [0; NONCE_BYTES];
	full[SYNC_NONCE_OFFSET..].copy_from_slice(&nonce.to_le_bytes());
	full
}

pub fn wrapped_generation(oldest: u32, generation: u32) -> u32 {
	let remainder = oldest % GENERATION_WRAP;
	let factor = (oldest / GENERATION_WRAP).wrapping_add(u32::from(generation < remainder));
	factor.wrapping_mul(GENERATION_WRAP).wrapping_add(generation)
}

pub fn write_leb128(mut value: u64, out: &mut Vec<u8>) {
	while value >= u64::from(LEB128_CONTINUE) {
		out.push(LEB128_CONTINUE | (value as u8 & LEB128_PAYLOAD_MASK));
		value >>= LEB128_PAYLOAD_BITS;
	}
	out.push(value as u8);
}

pub fn leb128_len(mut value: u64) -> usize {
	let mut len = 1;
	while value >= u64::from(LEB128_CONTINUE) {
		len += 1;
		value >>= LEB128_PAYLOAD_BITS;
	}
	len
}

fn read_leb128(bytes: &[u8]) -> Option<(u64, usize)> {
	let mut value = 0u64;
	for (index, byte) in bytes.iter().take(MAX_LEB128_BYTES).enumerate() {
		let shift = LEB128_PAYLOAD_BITS * index as u32;
		let payload = u64::from(byte & LEB128_PAYLOAD_MASK);
		if index == MAX_LEB128_BYTES - 1 && payload > 1 {
			return None;
		}
		value |= payload << shift;
		if byte & LEB128_CONTINUE == 0 {
			return Some((value, index + 1));
		}
	}
	None
}

pub fn seal(cipher: &Cipher, nonce: u32, media: &[u8], ranges: &[(usize, usize)]) -> Option<Vec<u8>> {
	let (associated, mut ciphertext) = split(media, ranges);
	let tag = cipher.seal(nonce, &mut ciphertext, &associated)?;
	let nonce_len = leb128_len(u64::from(nonce));
	let ranges_len: usize = ranges
		.iter()
		.map(|&(offset, size)| leb128_len(offset as u64) + leb128_len(size as u64))
		.sum();
	let supplemental = TAG_BYTES + nonce_len + ranges_len + SIZE_BYTES + MARKER.len();
	let mut frame = interleave(ranges, &associated, &ciphertext);
	frame.reserve(supplemental);
	frame.extend_from_slice(&tag);
	write_leb128(u64::from(nonce), &mut frame);
	for &(offset, size) in ranges {
		write_leb128(offset as u64, &mut frame);
		write_leb128(size as u64, &mut frame);
	}
	frame.push(u8::try_from(supplemental).ok()?);
	frame.extend_from_slice(&MARKER);
	Some(frame)
}

fn split(frame: &[u8], ranges: &[(usize, usize)]) -> (Vec<u8>, Vec<u8>) {
	let mut associated = Vec::new();
	let mut encrypted = Vec::with_capacity(frame.len());
	let mut index = 0;
	for &(offset, size) in ranges {
		encrypted.extend_from_slice(&frame[index..offset]);
		associated.extend_from_slice(&frame[offset..offset + size]);
		index = offset + size;
	}
	encrypted.extend_from_slice(&frame[index..]);
	(associated, encrypted)
}

fn interleave(ranges: &[(usize, usize)], associated: &[u8], encrypted: &[u8]) -> Vec<u8> {
	let mut frame = Vec::with_capacity(associated.len() + encrypted.len());
	let (mut index, mut encrypted_at, mut associated_at) = (0, 0, 0);
	for &(offset, size) in ranges {
		let gap = offset - index;
		frame.extend_from_slice(&encrypted[encrypted_at..encrypted_at + gap]);
		encrypted_at += gap;
		frame.extend_from_slice(&associated[associated_at..associated_at + size]);
		associated_at += size;
		index = offset + size;
	}
	frame.extend_from_slice(&encrypted[encrypted_at..]);
	frame
}

pub struct Parsed<'a> {
	pub nonce: u32,
	pub tag: Tag,
	body: &'a [u8],
	ranges: Vec<(usize, usize)>,
}

pub fn parse(frame: &[u8]) -> Option<Parsed<'_>> {
	if frame.len() < MIN_SUPPLEMENTAL_BYTES || !frame.ends_with(&MARKER) {
		return None;
	}
	let size_at = frame.len() - MARKER.len() - SIZE_BYTES;
	let supplemental = usize::from(frame[size_at]);
	if supplemental < MIN_SUPPLEMENTAL_BYTES || supplemental > frame.len() {
		return None;
	}
	let body_len = frame.len() - supplemental;
	let tag_at = body_len;
	let tag: Tag = frame[tag_at..tag_at + TAG_BYTES].try_into().ok()?;
	let mut cursor = tag_at + TAG_BYTES;
	let (nonce, used) = read_leb128(&frame[cursor..size_at])?;
	cursor += used;
	let ranges = read_ranges(&frame[cursor..size_at], body_len)?;
	Some(Parsed {
		nonce: nonce as u32,
		tag,
		body: &frame[..body_len],
		ranges,
	})
}

fn read_ranges(mut bytes: &[u8], body_len: usize) -> Option<Vec<(usize, usize)>> {
	let mut ranges: Vec<(usize, usize)> = Vec::new();
	let mut previous_end = 0usize;
	while !bytes.is_empty() {
		let (offset, used) = read_leb128(bytes)?;
		bytes = &bytes[used..];
		let (size, used) = read_leb128(bytes)?;
		bytes = &bytes[used..];
		let offset = usize::try_from(offset).ok()?;
		let end = offset.checked_add(usize::try_from(size).ok()?)?;
		if offset < previous_end || end > body_len {
			return None;
		}
		previous_end = end;
		ranges.push((offset, end - offset));
	}
	Some(ranges)
}

impl Parsed<'_> {
	pub fn open(&self, cipher: &Cipher) -> Option<Vec<u8>> {
		let (associated, mut ciphertext) = split(self.body, &self.ranges);
		cipher.open(self.nonce, &mut ciphertext, &associated, &self.tag)?;
		Some(interleave(&self.ranges, &associated, &ciphertext))
	}
}
