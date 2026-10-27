use std::collections::HashMap;
use std::fmt::Write as _;
use std::ops::Range;
use std::sync::LazyLock;

use bytes::Bytes;

const PACK: &[u8] = include_bytes!("../../../assets/twemoji.pack");
const MAGIC: &[u8; 4] = b"OXTW";
const SCHEME: &str = "twemoji:";
const ZWJ: char = '\u{200d}';
const EMOJI_VARIANT: char = '\u{fe0f}';
const TEXT_VARIANT: char = '\u{fe0e}';
const KEYCAP: char = '\u{20e3}';
const TEXT_DEFAULT: [char; 3] = ['\u{a9}', '\u{ae}', '\u{2122}'];

static INDEX: LazyLock<HashMap<&'static str, &'static [u8]>> = LazyLock::new(|| {
	let index = read_index(PACK);
	if index.is_empty() {
		tracing::warn!("the bundled twemoji pack is unreadable, emoji fall back to the system font");
	}
	index
});

fn read_u32(bytes: &[u8], at: usize) -> Option<usize> {
	let word = bytes.get(at..at + 4)?;
	Some(u32::from_le_bytes(word.try_into().ok()?) as usize)
}

fn read_index(pack: &'static [u8]) -> HashMap<&'static str, &'static [u8]> {
	let mut index = HashMap::new();
	if pack.get(..4) != Some(MAGIC.as_slice()) {
		return index;
	}
	let Some(count) = read_u32(pack, 4) else {
		return index;
	};
	let mut entries = Vec::with_capacity(count);
	let mut cursor = 8;
	for _ in 0..count {
		let Some(&length) = pack.get(cursor) else {
			return HashMap::new();
		};
		let key_end = cursor + 1 + usize::from(length);
		let (Some(key), Some(offset), Some(size)) = (
			pack.get(cursor + 1..key_end)
				.and_then(|key| std::str::from_utf8(key).ok()),
			read_u32(pack, key_end),
			read_u32(pack, key_end + 4),
		) else {
			return HashMap::new();
		};
		entries.push((key, offset, size));
		cursor = key_end + 8;
	}
	for (key, offset, size) in entries {
		let start = cursor + offset;
		if let Some(image) = pack.get(start..start + size) {
			index.insert(key, image);
		}
	}
	index
}

pub fn bytes(url: &str) -> Option<Bytes> {
	let key = url.strip_prefix(SCHEME)?;
	INDEX.get(key).map(|image| Bytes::from_static(image))
}

pub fn is_pack_url(url: &str) -> bool {
	url.starts_with(SCHEME)
}

pub fn url(emoji: &str) -> Option<String> {
	let chars: Vec<char> = emoji.chars().collect();
	resolve(&chars).map(|key| format!("{SCHEME}{key}"))
}

pub fn find(text: &str) -> Vec<(Range<usize>, String)> {
	if text.chars().all(|character| character < '\u{a9}') {
		return Vec::new();
	}
	let chars: Vec<(usize, char)> = text.char_indices().collect();
	let mut found = Vec::new();
	let mut position = 0;
	let mut sequence = Vec::new();
	while position < chars.len() {
		let (start, first) = chars[position];
		if !may_start(first, chars.get(position + 1).map(|(_, next)| *next)) {
			position += 1;
			continue;
		}
		let length = sequence_length(&chars[position..]);
		sequence.clear();
		sequence.extend(
			chars[position..position + length]
				.iter()
				.map(|(_, character)| *character),
		);
		let matched = (1..=length)
			.rev()
			.filter(|end| *end == length || sequence[*end] == ZWJ)
			.find_map(|end| resolve(&sequence[..end]).map(|key| (end, key)));
		match matched {
			Some((end, key)) => {
				let finish = chars.get(position + end).map_or(text.len(), |(at, _)| *at);
				found.push((start..finish, format!("{SCHEME}{key}")));
				position += end;
			}
			None => position += 1,
		}
	}
	found
}

fn may_start(first: char, next: Option<char>) -> bool {
	match first {
		'#' | '*' | '0'..='9' => matches!(next, Some(EMOJI_VARIANT | KEYCAP)),
		'\u{a9}' | '\u{ae}' => true,
		other => (('\u{203c}'..='\u{3299}').contains(&other) || other >= '\u{1f000}') && !is_modifier(other),
	}
}

fn is_modifier(character: char) -> bool {
	matches!(character, EMOJI_VARIANT | TEXT_VARIANT | ZWJ | KEYCAP | '\u{1f3fb}'..='\u{1f3ff}' | '\u{e0020}'..='\u{e007f}')
}

fn is_regional(character: char) -> bool {
	('\u{1f1e6}'..='\u{1f1ff}').contains(&character)
}

fn sequence_length(chars: &[(usize, char)]) -> usize {
	let mut length = 1;
	if is_regional(chars[0].1) {
		return if chars.get(1).is_some_and(|(_, next)| is_regional(*next)) {
			2
		} else {
			1
		};
	}
	while let Some((_, next)) = chars.get(length) {
		if *next == ZWJ {
			if chars
				.get(length + 1)
				.is_none_or(|(_, joined)| is_modifier(*joined))
			{
				break;
			}
			length += 2;
		} else if is_modifier(*next) && *next != TEXT_VARIANT {
			length += 1;
		} else {
			break;
		}
	}
	length
}

fn resolve(sequence: &[char]) -> Option<&'static str> {
	if sequence.get(1) == Some(&TEXT_VARIANT) || (sequence.len() == 1 && TEXT_DEFAULT.contains(&sequence[0]))
	{
		return None;
	}
	let joined = sequence.contains(&ZWJ);
	let exact = key_for(sequence, !joined);
	lookup(&exact).or_else(|| joined.then(|| lookup(&key_for(sequence, true))).flatten())
}

fn lookup(key: &str) -> Option<&'static str> {
	INDEX.get_key_value(key).map(|(key, _)| *key)
}

fn key_for(sequence: &[char], strip_variant: bool) -> String {
	let mut key = String::with_capacity(sequence.len() * 6);
	for character in sequence
		.iter()
		.filter(|character| !(strip_variant && **character == EMOJI_VARIANT))
	{
		if !key.is_empty() {
			key.push('-');
		}
		let _ = write!(key, "{:x}", u32::from(*character));
	}
	key
}
