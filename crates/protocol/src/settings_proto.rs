use base64::Engine;
use base64::alphabet::STANDARD as STANDARD_ALPHABET;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use crate::snowflake::Snowflake;

const PRELOADED_GUILD_FOLDERS: u32 = 14;
const FOLDERS: u32 = 1;
const POSITIONS: u32 = 2;
const FOLDER_GUILD_IDS: u32 = 1;
const FOLDER_ID: u32 = 2;
const FOLDER_NAME: u32 = 3;
const FOLDER_COLOR: u32 = 4;
const WRAPPED_VALUE: u32 = 1;

const LENIENT: GeneralPurpose = GeneralPurpose::new(
	&STANDARD_ALPHABET,
	GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtoGuildFolder {
	pub id: Option<i64>,
	pub name: Option<String>,
	pub color: Option<u64>,
	pub guild_ids: Vec<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtoGuildFolders {
	pub folders: Vec<ProtoGuildFolder>,
	pub positions: Vec<Snowflake>,
}

#[derive(Clone, Copy)]
enum Wire<'a> {
	Varint(u64),
	Fixed64(u64),
	Bytes(&'a [u8]),
	Skipped,
}

struct Fields<'a> {
	bytes: &'a [u8],
}

impl<'a> Fields<'a> {
	fn new(bytes: &'a [u8]) -> Self {
		Self { bytes }
	}

	fn varint(&mut self) -> Option<u64> {
		let mut value = 0u64;
		for (index, byte) in self.bytes.iter().enumerate().take(10) {
			value |= u64::from(byte & 0x7f) << (7 * index);
			if byte & 0x80 == 0 {
				self.bytes = &self.bytes[index + 1..];
				return Some(value);
			}
		}
		None
	}

	fn take(&mut self, length: usize) -> Option<&'a [u8]> {
		let (head, tail) = self.bytes.split_at_checked(length)?;
		self.bytes = tail;
		Some(head)
	}
}

impl<'a> Iterator for Fields<'a> {
	type Item = (u32, Wire<'a>);

	fn next(&mut self) -> Option<Self::Item> {
		if self.bytes.is_empty() {
			return None;
		}
		let key = self.varint()?;
		let number = u32::try_from(key >> 3).ok()?;
		let wire = match key & 7 {
			0 => Wire::Varint(self.varint()?),
			1 => Wire::Fixed64(u64::from_le_bytes(self.take(8)?.try_into().ok()?)),
			2 => {
				let length = usize::try_from(self.varint()?).ok()?;
				Wire::Bytes(self.take(length)?)
			}
			5 => {
				self.take(4)?;
				Wire::Skipped
			}
			_ => return None,
		};
		Some((number, wire))
	}
}

fn wrapped(bytes: &[u8]) -> Option<Wire<'_>> {
	Fields::new(bytes)
		.find(|(number, _)| *number == WRAPPED_VALUE)
		.map(|(_, wire)| wire)
}

fn push_ids(ids: &mut Vec<Snowflake>, wire: Wire) {
	match wire {
		Wire::Fixed64(id) => ids.push(Snowflake(id)),
		Wire::Bytes(packed) => ids.extend(
			packed
				.as_chunks::<8>()
				.0
				.iter()
				.map(|chunk| Snowflake(u64::from_le_bytes(*chunk))),
		),
		Wire::Varint(_) | Wire::Skipped => {}
	}
}

fn decode_folder(bytes: &[u8]) -> ProtoGuildFolder {
	let mut folder = ProtoGuildFolder::default();
	for (number, wire) in Fields::new(bytes) {
		match (number, wire) {
			(FOLDER_GUILD_IDS, wire) => push_ids(&mut folder.guild_ids, wire),
			(FOLDER_ID, Wire::Bytes(inner)) => {
				if let Some(Wire::Varint(id)) = wrapped(inner) {
					folder.id = Some(id as i64);
				}
			}
			(FOLDER_NAME, Wire::Bytes(inner)) => {
				if let Some(Wire::Bytes(name)) = wrapped(inner) {
					folder.name = Some(String::from_utf8_lossy(name).into_owned());
				}
			}
			(FOLDER_COLOR, Wire::Bytes(inner)) => {
				if let Some(Wire::Varint(color)) = wrapped(inner) {
					folder.color = Some(color);
				}
			}
			_ => {}
		}
	}
	folder
}

fn decode_folders(bytes: &[u8]) -> ProtoGuildFolders {
	let mut decoded = ProtoGuildFolders::default();
	for (number, wire) in Fields::new(bytes) {
		match (number, wire) {
			(FOLDERS, Wire::Bytes(inner)) => decoded.folders.push(decode_folder(inner)),
			(POSITIONS, wire) => push_ids(&mut decoded.positions, wire),
			_ => {}
		}
	}
	decoded
}

pub fn decode_guild_folders(encoded: &str) -> Option<ProtoGuildFolders> {
	let bytes = LENIENT.decode(encoded.trim()).ok()?;
	Fields::new(&bytes).find_map(|(number, wire)| match (number, wire) {
		(PRELOADED_GUILD_FOLDERS, Wire::Bytes(inner)) => Some(decode_folders(inner)),
		_ => None,
	})
}
