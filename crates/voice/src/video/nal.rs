pub const LONG_START_CODE: [u8; 4] = [0, 0, 0, 1];
pub const SLICE: u8 = 1;
pub const IDR: u8 = 5;
pub const SPS: u8 = 7;
pub const PPS: u8 = 8;
pub const HEADER_BYTES: usize = 1;

const SHORT_START_CODE_BYTES: usize = 3;
const TYPE_MASK: u8 = 0x1f;

pub fn unit_type(header: u8) -> u8 {
	header & TYPE_MASK
}

pub fn find_unit(buffer: &[u8], from: usize) -> Option<(usize, usize)> {
	if buffer.len() < SHORT_START_CODE_BYTES {
		return None;
	}
	let mut index = from;
	while index < buffer.len() - SHORT_START_CODE_BYTES {
		match buffer[index + 2] {
			0 => index += 1,
			1 => {
				if buffer[index] == 0 && buffer[index + 1] == 0 {
					let long = index >= 1 && buffer[index - 1] == 0;
					return Some((index + SHORT_START_CODE_BYTES, if long { 4 } else { 3 }));
				}
				index += SHORT_START_CODE_BYTES;
			}
			_ => index += SHORT_START_CODE_BYTES,
		}
	}
	None
}

pub fn units(frame: &[u8]) -> Vec<&[u8]> {
	let mut units = Vec::new();
	let mut current = find_unit(frame, 0);
	while let Some((start, _)) = current {
		let next = find_unit(frame, start);
		let end = next.map_or(frame.len(), |(next_start, code)| next_start - code);
		if start < end {
			units.push(&frame[start..end]);
		}
		current = next;
	}
	units
}

pub fn is_keyframe(frame: &[u8]) -> bool {
	units(frame).iter().any(|unit| unit_type(unit[0]) == IDR)
}

pub fn annex_b<'a>(units: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
	let mut frame = Vec::new();
	for unit in units {
		frame.extend_from_slice(&LONG_START_CODE);
		frame.extend_from_slice(unit);
	}
	frame
}
