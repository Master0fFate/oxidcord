pub const HEADER_LEN: usize = 12;
pub const OPUS_PAYLOAD_TYPE: u8 = 0x78;
pub const VIDEO_HEADER_LEN: usize = HEADER_LEN + EXTENSION_PREAMBLE_LEN;
pub const VIDEO_EXTENSION_LEN: usize = EXTENSION_WORD_LEN;

const VERSION_2: u8 = 0x80;
const MARKER_FLAG: u8 = 0x80;
const RTCP_PAYLOAD_TYPES: std::ops::RangeInclusive<u8> = 192..=223;
const ONE_BYTE_EXTENSION_PROFILE: [u8; 2] = [0xbe, 0xde];
const PLAYOUT_DELAY_ID: u8 = 6;
const PLAYOUT_DELAY_LEN: u8 = 3;
const VERSION_MASK: u8 = 0xc0;
const PADDING_FLAG: u8 = 0x20;
const EXTENSION_FLAG: u8 = 0x10;
const CSRC_COUNT_MASK: u8 = 0x0f;
const PAYLOAD_TYPE_MASK: u8 = 0x7f;
const CSRC_LEN: usize = 4;
pub const RTCP_HEADER_LEN: usize = 8;
const EXTENSION_PREAMBLE_LEN: usize = 4;
const EXTENSION_WORD_LEN: usize = 4;

pub fn write_header(sequence: u16, timestamp: u32, ssrc: u32) -> [u8; HEADER_LEN] {
	let mut header = [0u8; HEADER_LEN];
	header[0] = VERSION_2;
	header[1] = OPUS_PAYLOAD_TYPE;
	header[2..4].copy_from_slice(&sequence.to_be_bytes());
	header[4..8].copy_from_slice(&timestamp.to_be_bytes());
	header[8..12].copy_from_slice(&ssrc.to_be_bytes());
	header
}

pub fn write_video_header(
	payload_type: u8,
	marker: bool,
	sequence: u16,
	timestamp: u32,
	ssrc: u32,
) -> [u8; VIDEO_HEADER_LEN] {
	let mut header = [0u8; VIDEO_HEADER_LEN];
	header[0] = VERSION_2 | EXTENSION_FLAG;
	header[1] = payload_type | if marker { MARKER_FLAG } else { 0 };
	header[2..4].copy_from_slice(&sequence.to_be_bytes());
	header[4..8].copy_from_slice(&timestamp.to_be_bytes());
	header[8..12].copy_from_slice(&ssrc.to_be_bytes());
	header[12..14].copy_from_slice(&ONE_BYTE_EXTENSION_PROFILE);
	header[14..16].copy_from_slice(&1u16.to_be_bytes());
	header
}

pub const fn video_extension() -> [u8; VIDEO_EXTENSION_LEN] {
	[(PLAYOUT_DELAY_ID << 4) | (PLAYOUT_DELAY_LEN - 1), 0, 0, 0]
}

pub fn is_rtcp(packet: &[u8]) -> bool {
	packet.len() >= RTCP_HEADER_LEN
		&& packet[0] & VERSION_MASK == VERSION_2
		&& RTCP_PAYLOAD_TYPES.contains(&packet[1])
}

#[derive(Debug, Clone, Copy)]
pub struct Header {
	pub payload_type: u8,
	pub marker: bool,
	pub sequence: u16,
	pub timestamp: u32,
	pub ssrc: u32,
	pub header_len: usize,
	extension_len: usize,
	padded: bool,
}

pub fn parse(packet: &[u8]) -> Option<Header> {
	if packet.len() < HEADER_LEN || packet[0] & VERSION_MASK != VERSION_2 {
		return None;
	}
	let mut header_len = HEADER_LEN + CSRC_LEN * usize::from(packet[0] & CSRC_COUNT_MASK);
	let mut extension_len = 0;
	if packet[0] & EXTENSION_FLAG != 0 {
		let preamble = packet.get(header_len..header_len + EXTENSION_PREAMBLE_LEN)?;
		extension_len = usize::from(u16::from_be_bytes([preamble[2], preamble[3]])) * EXTENSION_WORD_LEN;
		header_len += EXTENSION_PREAMBLE_LEN;
	}
	if packet.len() < header_len {
		return None;
	}
	Some(Header {
		payload_type: packet[1] & PAYLOAD_TYPE_MASK,
		marker: packet[1] & MARKER_FLAG != 0,
		sequence: u16::from_be_bytes([packet[2], packet[3]]),
		timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
		ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
		header_len,
		extension_len,
		padded: packet[0] & PADDING_FLAG != 0,
	})
}

impl Header {
	pub fn into_payload(self, mut decrypted: Vec<u8>) -> Option<Vec<u8>> {
		if self.padded {
			let padding = usize::from(*decrypted.last()?);
			if padding == 0 || padding > decrypted.len() {
				return None;
			}
			decrypted.truncate(decrypted.len() - padding);
		}
		if self.extension_len > 0 {
			if self.extension_len > decrypted.len() {
				return None;
			}
			decrypted.drain(..self.extension_len);
		}
		(!decrypted.is_empty()).then_some(decrypted)
	}
}
