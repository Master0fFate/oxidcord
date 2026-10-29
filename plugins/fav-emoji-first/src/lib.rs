use plugin_sdk::model::{EmojiInfo, EmojiRef, OutgoingFull, SendDecisionFull};
use plugin_sdk::{Plugin, PluginExt, data, export_plugin, log, storage};

const RECENT_KEY: &str = "recent";
const MAX_RECENT: usize = 50;
const SKIN_TONES: std::ops::RangeInclusive<char> = '\u{1F3FB}'..='\u{1F3FF}';
const JOINER: char = '\u{200D}';
const VARIATION_SELECTOR: char = '\u{FE0F}';

#[derive(Default)]
struct FavEmojiFirst {
	recent: Vec<String>,
}

fn is_pictograph(character: char) -> bool {
	matches!(
		character,
		'\u{1F000}'..='\u{1FAFF}' | '\u{2600}'..='\u{27BF}' | '\u{2B00}'..='\u{2BFF}' | '\u{231A}'..='\u{23FF}'
	)
}

fn glyph_key(glyph: &str) -> String {
	glyph
		.chars()
		.filter(|&character| character != VARIATION_SELECTOR)
		.collect()
}

fn emoji_key(emoji: &EmojiRef) -> String {
	emoji
		.id
		.map_or_else(|| glyph_key(&emoji.name), |id| id.to_string())
}

fn custom_ids(content: &str) -> impl Iterator<Item = String> + '_ {
	content.match_indices('<').filter_map(|(at, _)| {
		let rest = content[at + 1..].strip_prefix('a').unwrap_or(&content[at + 1..]);
		let (name, rest) = rest.strip_prefix(':')?.split_once(':')?;
		let (id, _) = rest.split_once('>')?;
		let valid = !name.is_empty()
			&& name
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
			&& !id.is_empty()
			&& id.bytes().all(|byte| byte.is_ascii_digit());
		valid.then(|| id.to_owned())
	})
}

fn unicode_glyphs(content: &str) -> Vec<String> {
	let mut glyphs = Vec::new();
	let mut characters = content.chars().peekable();
	while let Some(character) = characters.next() {
		if !is_pictograph(character) {
			continue;
		}
		let mut glyph = String::from(character);
		while let Some(&next) = characters.peek() {
			let joined = next == VARIATION_SELECTOR || SKIN_TONES.contains(&next);
			if joined {
				glyph.push(next);
				characters.next();
			} else if next == JOINER {
				characters.next();
				match characters.next_if(|&after| is_pictograph(after)) {
					Some(after) => {
						glyph.push(JOINER);
						glyph.push(after);
					}
					None => break,
				}
			} else {
				break;
			}
		}
		glyphs.push(glyph_key(&glyph));
	}
	glyphs
}

impl FavEmojiFirst {
	fn remember(&mut self, keys: Vec<String>) -> bool {
		let before = self.recent.clone();
		for key in keys {
			self.recent.retain(|known| *known != key);
			self.recent.insert(0, key);
		}
		self.recent.truncate(MAX_RECENT);
		self.recent != before
	}

	fn save(&self) {
		let Ok(bytes) = serde_json::to_vec(&self.recent) else {
			return;
		};
		if let Err(error) = storage::set(RECENT_KEY, &bytes) {
			log::warn!("could not save the recent emoji: {error:?}");
		}
	}
}

impl Plugin for FavEmojiFirst {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(bytes)) = storage::get(RECENT_KEY) {
			self.recent = serde_json::from_slice(&bytes).unwrap_or_default();
		}
		Ok(())
	}
}

impl PluginExt for FavEmojiFirst {
	fn before_send_full(&mut self, msg: OutgoingFull) -> SendDecisionFull {
		let mut keys: Vec<String> = custom_ids(&msg.content).collect();
		keys.extend(unicode_glyphs(&msg.content));
		keys.reverse();
		if !keys.is_empty() && self.remember(keys) {
			self.save();
		}
		SendDecisionFull::Pass
	}

	fn autocomplete_emoji(&mut self, _query: &str, candidates: &[EmojiInfo]) -> Vec<u32> {
		let favorites: Vec<String> = data::list_favorite_emojis()
			.unwrap_or_default()
			.iter()
			.map(emoji_key)
			.collect();
		let rank = |info: &EmojiInfo| {
			let key = emoji_key(&info.emoji);
			favorites
				.iter()
				.position(|favorite| *favorite == key)
				.or_else(|| {
					self.recent
						.iter()
						.position(|recent| *recent == key)
						.map(|at| favorites.len() + at)
				})
		};
		let mut ranked: Vec<(usize, u32)> = Vec::new();
		let mut unranked: Vec<u32> = Vec::new();
		for (index, info) in candidates.iter().enumerate() {
			match rank(info) {
				Some(position) => ranked.push((position, index as u32)),
				None => unranked.push(index as u32),
			}
		}
		if ranked.is_empty() {
			return Vec::new();
		}
		ranked.sort_unstable();
		ranked
			.into_iter()
			.map(|(_, index)| index)
			.chain(unranked)
			.collect()
	}
}

export_plugin!(FavEmojiFirst, ext);
