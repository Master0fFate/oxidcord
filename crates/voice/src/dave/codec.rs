use crate::video::nal::{self, HEADER_BYTES, LONG_START_CODE};

const MAX_GOLOMB_ZERO_BITS: usize = 32;
const SLICE_HEADER_VALUES: u32 = 3;
const EMULATION_PREVENTION: u8 = 3;
const MIN_FRAME_BYTES: usize = 4;
const BOUNDARY_PADDING: usize = 2;

pub type Range = (usize, usize);

pub struct Processed {
	pub frame: Vec<u8>,
	pub ranges: Vec<Range>,
}

impl Processed {
	fn with_capacity(capacity: usize) -> Self {
		Self {
			frame: Vec::with_capacity(capacity),
			ranges: Vec::new(),
		}
	}

	fn keep(&mut self, bytes: &[u8]) {
		match self.ranges.last_mut() {
			Some((offset, size)) if *offset + *size == self.frame.len() => *size += bytes.len(),
			_ => self.ranges.push((self.frame.len(), bytes.len())),
		}
		self.frame.extend_from_slice(bytes);
	}

	fn protect(&mut self, bytes: &[u8]) {
		self.frame.extend_from_slice(bytes);
	}
}

pub fn process_h264(frame: &[u8]) -> Option<Processed> {
	if frame.len() < MIN_FRAME_BYTES {
		return None;
	}
	let mut processed = Processed::with_capacity(frame.len() + LONG_START_CODE.len());
	let mut current = nal::find_unit(frame, 0);
	while let Some((start, _)) = current {
		if start >= frame.len() - 1 {
			break;
		}
		let next = nal::find_unit(frame, start);
		let end = next.map_or(frame.len(), |(next_start, code)| next_start - code);
		processed.keep(&LONG_START_CODE);
		let kind = nal::unit_type(frame[start]);
		if kind == nal::SLICE || kind == nal::IDR {
			let payload = start + HEADER_BYTES;
			let clear_end = (payload + bytes_covering_slice_header(&frame[payload..])).min(end);
			processed.keep(&frame[start..clear_end]);
			processed.protect(&frame[clear_end..end]);
		} else {
			processed.keep(&frame[start..end]);
		}
		current = next;
	}
	Some(processed)
}

fn bytes_covering_slice_header(payload: &[u8]) -> usize {
	let total_bits = payload.len() * 8;
	let mut bit = 0;
	let mut zeros = 0;
	let mut parsed = 0;
	while bit < total_bits && parsed < SLICE_HEADER_VALUES {
		let byte_index = bit / 8;
		let bit_index = bit % 8;
		let byte = payload[byte_index];
		if bit_index == 0
			&& byte_index >= 2
			&& byte == EMULATION_PREVENTION
			&& payload[byte_index - 1] == 0
			&& payload[byte_index - 2] == 0
		{
			bit += 8;
			continue;
		}
		if byte & (1 << (7 - bit_index)) == 0 {
			zeros += 1;
			bit += 1;
			if zeros >= MAX_GOLOMB_ZERO_BITS {
				return 0;
			}
		} else {
			parsed += 1;
			bit += 1 + zeros;
			zeros = 0;
		}
	}
	bit / 8 + 1
}

pub fn ciphertext_is_packetizable(frame: &[u8], ranges: &[Range]) -> bool {
	let mut section_start = 0;
	for &(offset, size) in ranges {
		if section_start == offset {
			section_start += size;
			continue;
		}
		let start = section_start.saturating_sub(BOUNDARY_PADDING);
		let end = (offset + BOUNDARY_PADDING).min(frame.len());
		if nal::find_unit(&frame[start..end], 0).is_some() {
			return false;
		}
		section_start = offset + size;
	}
	if section_start == frame.len() {
		return true;
	}
	let start = section_start.saturating_sub(BOUNDARY_PADDING);
	nal::find_unit(&frame[start..], 0).is_none()
}
