use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

use super::unicode_table::TABLE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmojiCategory {
	People,
	Nature,
	Food,
	Activities,
	Travel,
	Objects,
	Symbols,
	Flags,
}

impl EmojiCategory {
	pub const ALL: [EmojiCategory; 8] = [
		Self::People,
		Self::Nature,
		Self::Food,
		Self::Activities,
		Self::Travel,
		Self::Objects,
		Self::Symbols,
		Self::Flags,
	];

	pub fn name(self) -> &'static str {
		match self {
			Self::People => "people",
			Self::Nature => "nature",
			Self::Food => "food",
			Self::Activities => "activities",
			Self::Travel => "travel",
			Self::Objects => "objects",
			Self::Symbols => "symbols",
			Self::Flags => "flags",
		}
	}
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnicodeEmoji {
	pub shortcode: &'static str,
	pub aliases: &'static [&'static str],
	pub keywords: &'static [&'static str],
	pub emoji: &'static str,
	pub category: EmojiCategory,
	pub skin_tone_capable: bool,
}

impl UnicodeEmoji {
	pub fn names(&self) -> impl Iterator<Item = &'static str> {
		std::iter::once(self.shortcode).chain(self.aliases.iter().copied())
	}
}

static BY_NAME: LazyLock<HashMap<&'static str, &'static UnicodeEmoji>> = LazyLock::new(|| {
	let mut by_name = HashMap::with_capacity(TABLE.len() * 2);
	for entry in &TABLE {
		for name in entry.names() {
			by_name.entry(name).or_insert(entry);
		}
	}
	by_name
});

fn normalize(query: &str) -> Cow<'_, str> {
	let trimmed = query.trim().trim_matches(':');
	if trimmed
		.bytes()
		.any(|byte| byte.is_ascii_uppercase() || byte == b' ')
	{
		Cow::Owned(trimmed.to_lowercase().replace(' ', "_"))
	} else {
		Cow::Borrowed(trimmed)
	}
}

fn rank(entry: &UnicodeEmoji, needle: &str) -> Option<u8> {
	let named = entry.names().filter_map(|name| {
		if name == needle {
			Some(0)
		} else if name.starts_with(needle) {
			Some(1)
		} else {
			name.contains(needle).then_some(2)
		}
	});
	let keyword = entry
		.keywords
		.iter()
		.any(|keyword| keyword.contains(needle))
		.then_some(3);
	named.chain(keyword).min()
}

pub fn unicode_emoji_list() -> &'static [UnicodeEmoji] {
	&TABLE
}

pub fn by_shortcode(name: &str) -> Option<&'static UnicodeEmoji> {
	BY_NAME.get(normalize(name).as_ref()).copied()
}

pub fn search_unicode(query: &str) -> Vec<&'static UnicodeEmoji> {
	let needle = normalize(query);
	if needle.is_empty() {
		return Vec::new();
	}
	let mut ranked: Vec<(u8, &'static UnicodeEmoji)> = TABLE
		.iter()
		.filter_map(|entry| rank(entry, &needle).map(|rank| (rank, entry)))
		.collect();
	ranked.sort_by_key(|(rank, _)| *rank);
	ranked.into_iter().map(|(_, entry)| entry).collect()
}
