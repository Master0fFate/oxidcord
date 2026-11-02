use base64::Engine;
use base64::alphabet::STANDARD as STANDARD_ALPHABET;
use base64::engine::general_purpose::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use crate::snowflake::Snowflake;

const PRELOADED_PRIVACY: u32 = 8;
const PRELOADED_STATUS: u32 = 11;
const PRELOADED_GUILD_FOLDERS: u32 = 14;
const STATUS_VALUE: u32 = 1;
const STATUS_CUSTOM: u32 = 2;
const STATUS_SHOW_CURRENT_GAME: u32 = 3;
const CUSTOM_TEXT: u32 = 1;
const CUSTOM_EMOJI_ID: u32 = 2;
const CUSTOM_EMOJI_NAME: u32 = 3;
const CUSTOM_EXPIRES_AT_MS: u32 = 4;
const PRIVACY_FRIEND_SOURCE_FLAGS: u32 = 11;
const FOLDERS: u32 = 1;
const POSITIONS: u32 = 2;
const FOLDER_GUILD_IDS: u32 = 1;
const FOLDER_ID: u32 = 2;
const FOLDER_NAME: u32 = 3;
const FOLDER_COLOR: u32 = 4;
const WRAPPED_VALUE: u32 = 1;
const FRECENCY_VERSIONS: u32 = 1;
const FRECENCY_DATA_VERSION: u32 = 3;
const FRECENCY_FAVORITE_GIFS: u32 = 2;
const FAVORITES_ENTRIES: u32 = 1;
const FAVORITES_HIDE_TOOLTIP: u32 = 2;
const FAVORITE_KEY: u32 = 1;
const FAVORITE_VALUE: u32 = 2;
const FAVORITE_FORMAT: u32 = 1;
const FAVORITE_SRC: u32 = 2;
const FAVORITE_WIDTH: u32 = 3;
const FAVORITE_HEIGHT: u32 = 4;
const FAVORITE_ORDER: u32 = 5;

pub const FRIEND_SOURCE_MUTUAL_FRIENDS: u32 = 1 << 1;
pub const FRIEND_SOURCE_MUTUAL_GUILDS: u32 = 1 << 2;
pub const FRIEND_SOURCE_NO_RELATION: u32 = 1 << 3;
pub const FRIEND_SOURCES_ALL: u32 =
	FRIEND_SOURCE_MUTUAL_FRIENDS | FRIEND_SOURCE_MUTUAL_GUILDS | FRIEND_SOURCE_NO_RELATION;

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtoSettings {
	pub status_bytes: Option<Vec<u8>>,
	pub privacy_bytes: Option<Vec<u8>>,
	pub guild_folders: Option<ProtoGuildFolders>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtoCustomStatus {
	pub text: String,
	pub emoji_id: Option<Snowflake>,
	pub emoji_name: Option<String>,
	pub expires_at_ms: Option<u64>,
}

impl ProtoCustomStatus {
	pub fn expired_at(&self, now_ms: u64) -> bool {
		self.expires_at_ms.is_some_and(|expires| expires <= now_ms)
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoStatus {
	pub status: Option<String>,
	pub custom_status: Option<ProtoCustomStatus>,
	pub show_current_game: bool,
}

impl Default for ProtoStatus {
	fn default() -> Self {
		Self {
			status: None,
			custom_status: None,
			show_current_game: true,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtoFavoriteGif {
	pub url: String,
	pub format: u32,
	pub src: String,
	pub width: u32,
	pub height: u32,
	pub order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtoGifFavorites {
	pub gifs: Vec<ProtoFavoriteGif>,
	pub hide_tooltip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtoFrecencySettings {
	pub data_version: u32,
	pub favorites: ProtoGifFavorites,
}

#[derive(Clone, Copy)]
enum Wire<'a> {
	Varint(u64),
	Fixed64(u64),
	Fixed32(u32),
	Bytes(&'a [u8]),
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
			5 => Wire::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
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
		Wire::Varint(_) | Wire::Fixed32(_) => {}
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

fn wrapped_string(bytes: &[u8]) -> Option<String> {
	match wrapped(bytes)? {
		Wire::Bytes(text) => Some(String::from_utf8_lossy(text).into_owned()),
		_ => None,
	}
}

fn decode_custom_status(bytes: &[u8]) -> Option<ProtoCustomStatus> {
	let mut custom = ProtoCustomStatus::default();
	for (number, wire) in Fields::new(bytes) {
		match (number, wire) {
			(CUSTOM_TEXT, Wire::Bytes(text)) => custom.text = String::from_utf8_lossy(text).into_owned(),
			(CUSTOM_EMOJI_ID, Wire::Fixed64(id)) => {
				custom.emoji_id = Some(Snowflake(id)).filter(|id| id.0 != 0)
			}
			(CUSTOM_EMOJI_NAME, Wire::Bytes(name)) => {
				custom.emoji_name =
					Some(String::from_utf8_lossy(name).into_owned()).filter(|name| !name.is_empty());
			}
			(CUSTOM_EXPIRES_AT_MS, Wire::Fixed64(expires)) => {
				custom.expires_at_ms = Some(expires).filter(|expires| *expires != 0);
			}
			_ => {}
		}
	}
	(!custom.text.is_empty() || custom.emoji_id.is_some() || custom.emoji_name.is_some()).then_some(custom)
}

pub fn decode_status(bytes: &[u8]) -> ProtoStatus {
	let mut status = ProtoStatus::default();
	for (number, wire) in Fields::new(bytes) {
		match (number, wire) {
			(STATUS_VALUE, Wire::Bytes(inner)) => status.status = wrapped_string(inner),
			(STATUS_CUSTOM, Wire::Bytes(inner)) => status.custom_status = decode_custom_status(inner),
			(STATUS_SHOW_CURRENT_GAME, Wire::Bytes(inner)) => {
				status.show_current_game = matches!(wrapped(inner), Some(Wire::Varint(shown)) if shown != 0);
			}
			_ => {}
		}
	}
	status
}

pub fn decode_friend_source_flags(bytes: &[u8]) -> Option<u32> {
	Fields::new(bytes)
		.filter_map(|(number, wire)| match (number, wire) {
			(PRIVACY_FRIEND_SOURCE_FLAGS, Wire::Bytes(inner)) => Some(match wrapped(inner) {
				Some(Wire::Varint(flags)) => u32::try_from(flags).unwrap_or_default(),
				_ => 0,
			}),
			_ => None,
		})
		.last()
}

pub fn decode_settings(encoded: &str) -> Option<ProtoSettings> {
	let bytes = LENIENT.decode(encoded.trim()).ok()?;
	let mut settings = ProtoSettings::default();
	for (number, wire) in Fields::new(&bytes) {
		match (number, wire) {
			(PRELOADED_STATUS, Wire::Bytes(inner)) => settings.status_bytes = Some(inner.to_vec()),
			(PRELOADED_PRIVACY, Wire::Bytes(inner)) => settings.privacy_bytes = Some(inner.to_vec()),
			(PRELOADED_GUILD_FOLDERS, Wire::Bytes(inner)) => {
				settings.guild_folders = Some(decode_folders(inner));
			}
			_ => {}
		}
	}
	Some(settings)
}

fn decode_favorite(bytes: &[u8]) -> Option<ProtoFavoriteGif> {
	let mut gif = ProtoFavoriteGif::default();
	for (number, wire) in Fields::new(bytes) {
		match (number, wire) {
			(FAVORITE_FORMAT, Wire::Varint(format)) => gif.format = u32::try_from(format).unwrap_or_default(),
			(FAVORITE_SRC, Wire::Bytes(src)) => gif.src = String::from_utf8_lossy(src).into_owned(),
			(FAVORITE_WIDTH, Wire::Varint(width)) => gif.width = u32::try_from(width).unwrap_or_default(),
			(FAVORITE_HEIGHT, Wire::Varint(height)) => gif.height = u32::try_from(height).unwrap_or_default(),
			(FAVORITE_ORDER, Wire::Varint(order)) => gif.order = u32::try_from(order).unwrap_or_default(),
			_ => {}
		}
	}
	Some(gif).filter(|gif| !gif.src.is_empty())
}

fn decode_favorite_entry(bytes: &[u8]) -> Option<ProtoFavoriteGif> {
	let mut url = String::new();
	let mut gif = None;
	for (number, wire) in Fields::new(bytes) {
		match (number, wire) {
			(FAVORITE_KEY, Wire::Bytes(key)) => url = String::from_utf8_lossy(key).into_owned(),
			(FAVORITE_VALUE, Wire::Bytes(value)) => gif = decode_favorite(value),
			_ => {}
		}
	}
	Some(gif?.with_url(url)).filter(|gif| !gif.url.is_empty())
}

impl ProtoFavoriteGif {
	fn with_url(mut self, url: String) -> Self {
		self.url = url;
		self
	}
}

fn decode_favorites(bytes: &[u8]) -> ProtoGifFavorites {
	let mut favorites = ProtoGifFavorites::default();
	for (number, wire) in Fields::new(bytes) {
		match (number, wire) {
			(FAVORITES_ENTRIES, Wire::Bytes(entry)) => {
				let Some(gif) = decode_favorite_entry(entry) else {
					continue;
				};
				match favorites.gifs.iter_mut().find(|known| known.url == gif.url) {
					Some(known) => *known = gif,
					None => favorites.gifs.push(gif),
				}
			}
			(FAVORITES_HIDE_TOOLTIP, Wire::Varint(hidden)) => favorites.hide_tooltip = hidden != 0,
			_ => {}
		}
	}
	favorites
}

pub fn decode_frecency_settings(encoded: &str) -> Option<ProtoFrecencySettings> {
	let bytes = LENIENT.decode(encoded.trim()).ok()?;
	let mut settings = ProtoFrecencySettings::default();
	for (number, wire) in Fields::new(&bytes) {
		match (number, wire) {
			(FRECENCY_VERSIONS, Wire::Bytes(inner)) => {
				for (number, wire) in Fields::new(inner) {
					if let (FRECENCY_DATA_VERSION, Wire::Varint(version)) = (number, wire) {
						settings.data_version = u32::try_from(version).unwrap_or_default();
					}
				}
			}
			(FRECENCY_FAVORITE_GIFS, Wire::Bytes(inner)) => settings.favorites = decode_favorites(inner),
			_ => {}
		}
	}
	Some(settings)
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
	while value >= 0x80 {
		out.push((value as u8) | 0x80);
		value >>= 7;
	}
	out.push(value as u8);
}

fn put_tag(out: &mut Vec<u8>, number: u32, wire: u8) {
	put_varint(out, (u64::from(number) << 3) | u64::from(wire));
}

fn put_uint(out: &mut Vec<u8>, number: u32, value: u64) {
	if value != 0 {
		put_tag(out, number, 0);
		put_varint(out, value);
	}
}

fn put_bytes(out: &mut Vec<u8>, number: u32, bytes: &[u8]) {
	put_tag(out, number, 2);
	put_varint(out, bytes.len() as u64);
	out.extend_from_slice(bytes);
}

fn put_string(out: &mut Vec<u8>, number: u32, text: &str) {
	if !text.is_empty() {
		put_bytes(out, number, text.as_bytes());
	}
}

fn put_fixed64(out: &mut Vec<u8>, number: u32, value: u64) {
	if value != 0 {
		put_tag(out, number, 1);
		out.extend_from_slice(&value.to_le_bytes());
	}
}

fn put_wire(out: &mut Vec<u8>, number: u32, wire: Wire) {
	match wire {
		Wire::Varint(value) => {
			put_tag(out, number, 0);
			put_varint(out, value);
		}
		Wire::Fixed64(value) => {
			put_tag(out, number, 1);
			out.extend_from_slice(&value.to_le_bytes());
		}
		Wire::Fixed32(value) => {
			put_tag(out, number, 5);
			out.extend_from_slice(&value.to_le_bytes());
		}
		Wire::Bytes(bytes) => put_bytes(out, number, bytes),
	}
}

fn uint_wrapper(value: u64) -> Vec<u8> {
	let mut inner = Vec::new();
	put_uint(&mut inner, WRAPPED_VALUE, value);
	inner
}

fn string_wrapper(text: &str) -> Vec<u8> {
	let mut inner = Vec::new();
	put_string(&mut inner, WRAPPED_VALUE, text);
	inner
}

fn packed_ids(ids: &[Snowflake]) -> Vec<u8> {
	ids.iter().flat_map(|id| id.0.to_le_bytes()).collect()
}

fn with_field(message: &[u8], number: u32, field: Option<&[u8]>) -> Vec<u8> {
	let mut out = Vec::with_capacity(message.len());
	for (current, wire) in Fields::new(message).filter(|(current, _)| *current != number) {
		put_wire(&mut out, current, wire);
	}
	if let Some(field) = field {
		put_bytes(&mut out, number, field);
	}
	out
}

fn encode_custom_status(custom: &ProtoCustomStatus) -> Vec<u8> {
	let mut out = Vec::new();
	put_string(&mut out, CUSTOM_TEXT, &custom.text);
	put_fixed64(&mut out, CUSTOM_EMOJI_ID, custom.emoji_id.map_or(0, |id| id.0));
	put_string(
		&mut out,
		CUSTOM_EMOJI_NAME,
		custom.emoji_name.as_deref().unwrap_or_default(),
	);
	put_fixed64(
		&mut out,
		CUSTOM_EXPIRES_AT_MS,
		custom.expires_at_ms.unwrap_or_default(),
	);
	out
}

pub fn encode_status(status: &ProtoStatus) -> Vec<u8> {
	let mut out = Vec::new();
	if let Some(value) = &status.status {
		put_bytes(&mut out, STATUS_VALUE, &string_wrapper(value));
	}
	if let Some(custom) = &status.custom_status {
		put_bytes(&mut out, STATUS_CUSTOM, &encode_custom_status(custom));
	}
	put_bytes(
		&mut out,
		STATUS_SHOW_CURRENT_GAME,
		&uint_wrapper(u64::from(status.show_current_game)),
	);
	out
}

pub fn status_with_custom(status: &[u8], custom: Option<&ProtoCustomStatus>) -> Vec<u8> {
	with_field(status, STATUS_CUSTOM, custom.map(encode_custom_status).as_deref())
}

pub fn status_with_show_current_game(status: &[u8], shown: bool) -> Vec<u8> {
	with_field(
		status,
		STATUS_SHOW_CURRENT_GAME,
		Some(&uint_wrapper(u64::from(shown))),
	)
}

pub fn privacy_with_friend_sources(privacy: &[u8], flags: u32) -> Vec<u8> {
	with_field(
		privacy,
		PRIVACY_FRIEND_SOURCE_FLAGS,
		Some(&uint_wrapper(u64::from(flags))),
	)
}

fn encode_folder(folder: &ProtoGuildFolder) -> Vec<u8> {
	let mut out = Vec::new();
	if !folder.guild_ids.is_empty() {
		put_bytes(&mut out, FOLDER_GUILD_IDS, &packed_ids(&folder.guild_ids));
	}
	if let Some(id) = folder.id {
		put_bytes(&mut out, FOLDER_ID, &uint_wrapper(id as u64));
	}
	if let Some(name) = &folder.name {
		put_bytes(&mut out, FOLDER_NAME, &string_wrapper(name));
	}
	if let Some(color) = folder.color {
		put_bytes(&mut out, FOLDER_COLOR, &uint_wrapper(color));
	}
	out
}

fn encode_folders(folders: &ProtoGuildFolders) -> Vec<u8> {
	let mut out = Vec::new();
	for folder in &folders.folders {
		put_bytes(&mut out, FOLDERS, &encode_folder(folder));
	}
	if !folders.positions.is_empty() {
		put_bytes(&mut out, POSITIONS, &packed_ids(&folders.positions));
	}
	out
}

pub fn encode_settings(settings: &ProtoSettings) -> String {
	let mut out = Vec::new();
	if let Some(privacy) = &settings.privacy_bytes {
		put_bytes(&mut out, PRELOADED_PRIVACY, privacy);
	}
	if let Some(status) = &settings.status_bytes {
		put_bytes(&mut out, PRELOADED_STATUS, status);
	}
	if let Some(folders) = &settings.guild_folders {
		put_bytes(&mut out, PRELOADED_GUILD_FOLDERS, &encode_folders(folders));
	}
	STANDARD.encode(out)
}

fn encode_favorite(gif: &ProtoFavoriteGif) -> Vec<u8> {
	let mut out = Vec::new();
	put_uint(&mut out, FAVORITE_FORMAT, u64::from(gif.format));
	put_string(&mut out, FAVORITE_SRC, &gif.src);
	put_uint(&mut out, FAVORITE_WIDTH, u64::from(gif.width));
	put_uint(&mut out, FAVORITE_HEIGHT, u64::from(gif.height));
	put_uint(&mut out, FAVORITE_ORDER, u64::from(gif.order));
	out
}

fn encode_favorite_entry(gif: &ProtoFavoriteGif) -> Vec<u8> {
	let mut out = Vec::new();
	put_string(&mut out, FAVORITE_KEY, &gif.url);
	put_bytes(&mut out, FAVORITE_VALUE, &encode_favorite(gif));
	out
}

fn encode_favorites(favorites: &ProtoGifFavorites) -> Vec<u8> {
	let mut out = Vec::new();
	for gif in &favorites.gifs {
		put_bytes(&mut out, FAVORITES_ENTRIES, &encode_favorite_entry(gif));
	}
	if favorites.hide_tooltip {
		put_uint(&mut out, FAVORITES_HIDE_TOOLTIP, 1);
	}
	out
}

pub fn encode_frecency_favorites(favorites: &ProtoGifFavorites) -> String {
	let mut out = Vec::new();
	put_bytes(&mut out, FRECENCY_FAVORITE_GIFS, &encode_favorites(favorites));
	STANDARD.encode(out)
}

pub fn encode_frecency_settings(settings: &ProtoFrecencySettings) -> String {
	let mut out = Vec::new();
	let mut versions = Vec::new();
	put_uint(
		&mut versions,
		FRECENCY_DATA_VERSION,
		u64::from(settings.data_version),
	);
	put_bytes(&mut out, FRECENCY_VERSIONS, &versions);
	put_bytes(
		&mut out,
		FRECENCY_FAVORITE_GIFS,
		&encode_favorites(&settings.favorites),
	);
	STANDARD.encode(out)
}
