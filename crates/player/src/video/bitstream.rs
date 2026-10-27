const START_CODE: [u8; 4] = [0, 0, 0, 1];
const AVC_HEADER: usize = 5;
const HEVC_HEADER: usize = 22;
const AVC_IDR: u8 = 5;
const AVC_SPS: u8 = 7;
const HEVC_IRAP_FIRST: u8 = 16;
const HEVC_IRAP_LAST: u8 = 23;
const HEVC_VPS: u8 = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
	Avc,
	Hevc,
}

pub struct Bitstream {
	family: Family,
	length_size: usize,
	parameter_sets: Vec<u8>,
	out: Vec<u8>,
}

fn push_unit(out: &mut Vec<u8>, unit: &[u8]) {
	out.extend_from_slice(&START_CODE);
	out.extend_from_slice(unit);
}

fn read_u16(data: &[u8], at: usize) -> Option<usize> {
	Some(usize::from(u16::from_be_bytes([
		*data.get(at)?,
		*data.get(at + 1)?,
	])))
}

impl Bitstream {
	pub fn avc(config: &[u8]) -> Option<Self> {
		let length_size = usize::from(*config.get(4)? & 3) + 1;
		let mut parameter_sets = Vec::new();
		let mut at = AVC_HEADER;
		let sps_count = usize::from(*config.get(at)? & 0x1f);
		at += 1;
		for _ in 0..sps_count {
			let length = read_u16(config, at)?;
			push_unit(&mut parameter_sets, config.get(at + 2..at + 2 + length)?);
			at += 2 + length;
		}
		let pps_count = usize::from(*config.get(at)?);
		at += 1;
		for _ in 0..pps_count {
			let length = read_u16(config, at)?;
			push_unit(&mut parameter_sets, config.get(at + 2..at + 2 + length)?);
			at += 2 + length;
		}
		Some(Self {
			family: Family::Avc,
			length_size,
			parameter_sets,
			out: Vec::new(),
		})
	}

	pub fn hevc(config: &[u8]) -> Option<Self> {
		let length_size = usize::from(*config.get(21)? & 3) + 1;
		let mut parameter_sets = Vec::new();
		let arrays = usize::from(*config.get(HEVC_HEADER)?);
		let mut at = HEVC_HEADER + 1;
		for _ in 0..arrays {
			at += 1;
			let count = read_u16(config, at)?;
			at += 2;
			for _ in 0..count {
				let length = read_u16(config, at)?;
				push_unit(&mut parameter_sets, config.get(at + 2..at + 2 + length)?);
				at += 2 + length;
			}
		}
		Some(Self {
			family: Family::Hevc,
			length_size,
			parameter_sets,
			out: Vec::new(),
		})
	}

	fn unit_type(&self, unit: &[u8]) -> u8 {
		match self.family {
			Family::Avc => unit.first().map_or(0, |byte| byte & 0x1f),
			Family::Hevc => unit.first().map_or(0, |byte| (byte >> 1) & 0x3f),
		}
	}

	fn is_random_access(&self, kind: u8) -> bool {
		match self.family {
			Family::Avc => kind == AVC_IDR,
			Family::Hevc => (HEVC_IRAP_FIRST..=HEVC_IRAP_LAST).contains(&kind),
		}
	}

	fn is_parameter_set(&self, kind: u8) -> bool {
		match self.family {
			Family::Avc => kind == AVC_SPS,
			Family::Hevc => kind == HEVC_VPS,
		}
	}

	fn units<'a>(&self, packet: &'a [u8]) -> impl Iterator<Item = &'a [u8]> {
		let length_size = self.length_size;
		let mut at = 0usize;
		std::iter::from_fn(move || {
			if at + length_size > packet.len() {
				return None;
			}
			let length = packet[at..at + length_size]
				.iter()
				.fold(0usize, |acc, byte| (acc << 8) | usize::from(*byte));
			let start = at + length_size;
			let end = (start + length).min(packet.len());
			at = end;
			Some(&packet[start..end])
		})
	}

	pub fn is_random_access_unit(&self, packet: &[u8]) -> bool {
		self.units(packet)
			.any(|unit| self.is_random_access(self.unit_type(unit)))
	}

	pub fn convert(&mut self, packet: &[u8]) -> &[u8] {
		let (mut random_access, mut has_parameters) = (false, false);
		for unit in self.units(packet) {
			let kind = self.unit_type(unit);
			random_access |= self.is_random_access(kind);
			has_parameters |= self.is_parameter_set(kind);
		}
		let mut out = std::mem::take(&mut self.out);
		out.clear();
		if random_access && !has_parameters {
			out.extend_from_slice(&self.parameter_sets);
		}
		for unit in self.units(packet) {
			push_unit(&mut out, unit);
		}
		self.out = out;
		&self.out
	}
}
