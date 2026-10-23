use super::nal;

pub const PAYLOAD_TYPE: u8 = 101;
pub const RTX_PAYLOAD_TYPE: u8 = 102;
pub const CLOCK_HZ: u32 = 90_000;
pub const MAX_PAYLOAD_BYTES: usize = 1200;

const STAP_A: u8 = 24;
const FU_A: u8 = 28;
const FU_INDICATOR_NRI_MASK: u8 = 0xe0;
const FU_START: u8 = 0x80;
const FU_END: u8 = 0x40;
const FU_HEADER_BYTES: usize = 2;
const STAP_SIZE_BYTES: usize = 2;
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const SEQUENCE_HALF_RANGE: u16 = 0x8000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
	pub data: Vec<u8>,
	pub marker: bool,
}

pub fn payload_frame(frame: &[u8]) -> Vec<Payload> {
	let units = nal::units(frame);
	let mut payloads = Vec::with_capacity(units.len());
	for unit in units {
		if unit.len() <= MAX_PAYLOAD_BYTES {
			payloads.push(Payload {
				data: unit.to_vec(),
				marker: false,
			});
		} else {
			fragment(unit, &mut payloads);
		}
	}
	if let Some(last) = payloads.last_mut() {
		last.marker = true;
	}
	payloads
}

fn fragment(unit: &[u8], payloads: &mut Vec<Payload>) {
	let indicator = (unit[0] & FU_INDICATOR_NRI_MASK) | FU_A;
	let kind = nal::unit_type(unit[0]);
	let body = &unit[nal::HEADER_BYTES..];
	let chunks = body.chunks(MAX_PAYLOAD_BYTES - FU_HEADER_BYTES);
	let count = chunks.len();
	for (index, chunk) in chunks.enumerate() {
		let mut header = kind;
		if index == 0 {
			header |= FU_START;
		}
		if index + 1 == count {
			header |= FU_END;
		}
		let mut data = Vec::with_capacity(FU_HEADER_BYTES + chunk.len());
		data.push(indicator);
		data.push(header);
		data.extend_from_slice(chunk);
		payloads.push(Payload { data, marker: false });
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledFrame {
	pub data: Vec<u8>,
	pub keyframe: bool,
	pub timestamp: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Depacketized {
	pub frame: Option<AssembledFrame>,
	pub dropped: u32,
	pub lost: bool,
}

pub struct H264Depacketizer {
	last_sequence: Option<u16>,
	timestamp: Option<u32>,
	frame: Vec<u8>,
	fragment_open: bool,
	broken: bool,
	waiting_for_keyframe: bool,
}

impl Default for H264Depacketizer {
	fn default() -> Self {
		Self {
			last_sequence: None,
			timestamp: None,
			frame: Vec::new(),
			fragment_open: false,
			broken: false,
			waiting_for_keyframe: true,
		}
	}
}

impl H264Depacketizer {
	pub fn waiting_for_keyframe(&self) -> bool {
		self.waiting_for_keyframe
	}

	pub fn require_keyframe(&mut self) {
		self.waiting_for_keyframe = true;
	}

	pub fn push(&mut self, sequence: u16, timestamp: u32, marker: bool, payload: &[u8]) -> Depacketized {
		let mut output = Depacketized::default();
		let mut gap = false;
		if let Some(last) = self.last_sequence {
			let step = sequence.wrapping_sub(last);
			if step == 0 || step >= SEQUENCE_HALF_RANGE {
				return output;
			}
			gap = step > 1;
		}
		self.last_sequence = Some(sequence);
		if gap {
			self.on_loss(&mut output);
		}
		if self.timestamp != Some(timestamp) {
			if !self.frame.is_empty() {
				output.dropped += 1;
				output.lost = true;
				self.waiting_for_keyframe = true;
			}
			self.frame.clear();
			self.fragment_open = false;
			self.broken = gap;
			self.timestamp = Some(timestamp);
		}
		if !self.broken {
			self.append(payload);
		}
		if marker {
			self.finish(timestamp, &mut output);
		}
		output
	}

	fn on_loss(&mut self, output: &mut Depacketized) {
		if !self.frame.is_empty() {
			output.dropped += 1;
		}
		self.frame.clear();
		self.fragment_open = false;
		self.broken = true;
		self.waiting_for_keyframe = true;
		output.lost = true;
	}

	fn append(&mut self, payload: &[u8]) {
		let Some(&header) = payload.first() else {
			return;
		};
		match nal::unit_type(header) {
			1..=23 => self.append_unit(payload),
			STAP_A => self.append_aggregate(&payload[nal::HEADER_BYTES..]),
			FU_A => self.append_fragment(payload),
			_ => {}
		}
		if self.frame.len() > MAX_FRAME_BYTES {
			self.frame.clear();
			self.broken = true;
			self.waiting_for_keyframe = true;
		}
	}

	fn append_unit(&mut self, unit: &[u8]) {
		self.frame.extend_from_slice(&nal::LONG_START_CODE);
		self.frame.extend_from_slice(unit);
	}

	fn append_aggregate(&mut self, mut body: &[u8]) {
		while let Some((size, rest)) = body.split_first_chunk::<STAP_SIZE_BYTES>() {
			let size = usize::from(u16::from_be_bytes(*size));
			if size == 0 || size > rest.len() {
				self.broken = true;
				return;
			}
			self.append_unit(&rest[..size]);
			body = &rest[size..];
		}
	}

	fn append_fragment(&mut self, payload: &[u8]) {
		let Some((&[indicator, header], body)) = payload.split_first_chunk::<FU_HEADER_BYTES>() else {
			return;
		};
		if header & FU_START != 0 {
			self.frame.extend_from_slice(&nal::LONG_START_CODE);
			self.frame
				.push((indicator & FU_INDICATOR_NRI_MASK) | nal::unit_type(header));
			self.fragment_open = true;
		} else if !self.fragment_open {
			self.broken = true;
			return;
		}
		self.frame.extend_from_slice(body);
		if header & FU_END != 0 {
			self.fragment_open = false;
		}
	}

	fn finish(&mut self, timestamp: u32, output: &mut Depacketized) {
		let data = std::mem::take(&mut self.frame);
		let broken = std::mem::take(&mut self.broken) || self.fragment_open;
		self.fragment_open = false;
		self.timestamp = None;
		if broken || data.is_empty() {
			output.dropped += 1;
			self.waiting_for_keyframe = true;
			output.lost = true;
			return;
		}
		let keyframe = nal::is_keyframe(&data);
		if self.waiting_for_keyframe && !keyframe {
			output.dropped += 1;
			return;
		}
		if keyframe {
			self.waiting_for_keyframe = false;
		}
		output.frame = Some(AssembledFrame {
			data,
			keyframe,
			timestamp,
		});
	}
}
