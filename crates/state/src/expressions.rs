use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock};

use protocol::{
	Emoji, EmojiCategory, Gif, GifFavorites, GifTrending, GuildStickersUpdate, Permissions, PremiumType,
	Snowflake, Sticker, StickerPack, UnicodeEmoji, unicode_emoji_list,
};
use serde::{Deserialize, Serialize};

use crate::changes::Changes;
use crate::permissions::DM_PERMISSIONS;
use crate::state::State;

pub const EXPRESSIONS_KEY: Snowflake = Snowflake(0);
pub const RECENT_CAP: usize = 50;
const QUERY_CACHE_CAP: usize = 20;

static UNICODE_SECTIONS: LazyLock<Vec<UnicodeSection>> = LazyLock::new(|| {
	EmojiCategory::ALL
		.iter()
		.map(|&category| UnicodeSection {
			category,
			emojis: unicode_emoji_list()
				.iter()
				.filter(|emoji| emoji.category == category)
				.collect(),
		})
		.collect()
});

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmojiRef {
	pub name: String,
	pub id: Option<Snowflake>,
	pub animated: bool,
}

impl EmojiRef {
	pub fn is_same(&self, other: &EmojiRef) -> bool {
		match (self.id, other.id) {
			(Some(left), Some(right)) => left == right,
			(None, None) => self.name == other.name,
			_ => false,
		}
	}
}

#[derive(Debug)]
pub struct UnicodeSection {
	pub category: EmojiCategory,
	pub emojis: Vec<&'static UnicodeEmoji>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEmoji {
	pub id: Snowflake,
	pub name: String,
	pub animated: bool,
	pub available: bool,
	pub guild_id: Snowflake,
	pub external: bool,
	pub usable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildEmojis {
	pub guild_id: Snowflake,
	pub name: String,
	pub icon: Option<String>,
	pub emojis: Vec<CatalogEmoji>,
}

#[derive(Debug)]
pub struct EmojiCatalog {
	pub unicode: &'static [UnicodeSection],
	pub guilds: Vec<GuildEmojis>,
}

#[derive(Debug)]
pub(crate) struct QueryCache<T> {
	entries: VecDeque<(String, T)>,
}

impl<T> Default for QueryCache<T> {
	fn default() -> Self {
		Self {
			entries: VecDeque::new(),
		}
	}
}

impl<T> QueryCache<T> {
	pub(crate) fn get(&self, query: &str) -> Option<&T> {
		self.entries
			.iter()
			.find_map(|(cached, value)| (cached == query).then_some(value))
	}

	pub(crate) fn put(&mut self, query: &str, value: T) {
		if let Some(position) = self.entries.iter().position(|(cached, _)| cached == query) {
			self.entries.remove(position);
		} else if self.entries.len() >= QUERY_CACHE_CAP {
			self.entries.pop_front();
		}
		self.entries.push_back((query.to_owned(), value));
	}
}

#[derive(Debug)]
pub(crate) struct FavoriteGifs {
	pub(crate) version: u32,
	pub(crate) gifs: Arc<[Gif]>,
}

impl From<&GifFavorites> for FavoriteGifs {
	fn from(favorites: &GifFavorites) -> Self {
		Self {
			version: favorites.version,
			gifs: favorites.gifs.as_slice().into(),
		}
	}
}

#[derive(Debug, Default)]
pub(crate) struct Expressions {
	pub(crate) gif_trending: Option<Arc<GifTrending>>,
	pub(crate) gif_favorites: Option<FavoriteGifs>,
	pub(crate) gif_searches: QueryCache<Arc<[Gif]>>,
	pub(crate) gif_suggestions: QueryCache<Vec<String>>,
	pub(crate) sticker_packs: Option<Vec<StickerPack>>,
	pub(crate) guild_stickers: HashMap<Snowflake, Vec<Sticker>>,
	pub(crate) recent_emoji: Vec<EmojiRef>,
	pub(crate) recent_gifs: Vec<Gif>,
	pub(crate) recent_stickers: Vec<Sticker>,
}

fn emoji_usable(
	owner: Snowflake,
	animated: bool,
	available: bool,
	context: Option<Snowflake>,
	nitro: bool,
	permissions: Permissions,
) -> bool {
	if !available || (animated && !nitro) {
		return false;
	}
	context == Some(owner) || (nitro && permissions.contains(Permissions::USE_EXTERNAL_EMOJIS))
}

fn catalog_emoji(
	emoji: &Emoji,
	owner: Snowflake,
	context: Option<Snowflake>,
	nitro: bool,
	permissions: Permissions,
) -> Option<CatalogEmoji> {
	let animated = emoji.animated == Some(true);
	let available = emoji.available != Some(false);
	Some(CatalogEmoji {
		id: emoji.id?,
		name: emoji.name.clone()?,
		animated,
		available,
		guild_id: owner,
		external: context != Some(owner),
		usable: emoji_usable(owner, animated, available, context, nitro, permissions),
	})
}

impl State {
	pub fn gif_trending(&self) -> Option<&Arc<GifTrending>> {
		self.lazy.expressions.gif_trending.as_ref()
	}

	pub fn gif_favorites(&self) -> Option<&Arc<[Gif]>> {
		self.lazy
			.expressions
			.gif_favorites
			.as_ref()
			.map(|favorites| &favorites.gifs)
	}

	pub fn gif_search(&self, query: &str) -> Option<&Arc<[Gif]>> {
		self.lazy.expressions.gif_searches.get(query)
	}

	pub fn gif_suggestions(&self, query: &str) -> Option<&[String]> {
		self.lazy
			.expressions
			.gif_suggestions
			.get(query)
			.map(Vec::as_slice)
	}

	pub fn sticker_packs(&self) -> Option<&[StickerPack]> {
		self.lazy.expressions.sticker_packs.as_deref()
	}

	pub fn guild_stickers(&self, guild_id: Snowflake) -> Option<&[Sticker]> {
		self.lazy
			.expressions
			.guild_stickers
			.get(&guild_id)
			.map(Vec::as_slice)
	}

	pub fn find_sticker(&self, sticker_id: Snowflake) -> Option<&Sticker> {
		let expressions = &self.lazy.expressions;
		expressions
			.sticker_packs
			.iter()
			.flatten()
			.flat_map(|pack| pack.stickers.iter())
			.chain(expressions.guild_stickers.values().flatten())
			.find(|sticker| sticker.id == sticker_id)
	}

	pub fn recent_emoji(&self) -> &[EmojiRef] {
		&self.lazy.expressions.recent_emoji
	}

	pub fn recent_gifs(&self) -> &[Gif] {
		&self.lazy.expressions.recent_gifs
	}

	pub fn recent_stickers(&self) -> &[Sticker] {
		&self.lazy.expressions.recent_stickers
	}

	pub fn has_nitro(&self) -> bool {
		self.current_user
			.as_ref()
			.and_then(|user| user.premium_type)
			.is_some_and(|kind| kind != PremiumType::None)
	}

	pub fn emoji_catalog(&self, guild_id: Option<Snowflake>) -> EmojiCatalog {
		let nitro = self.has_nitro();
		let permissions = guild_id
			.zip(self.current_user_id())
			.map_or(DM_PERMISSIONS, |(guild_id, me)| {
				self.guild_permissions(guild_id, me)
			});
		let current = guild_id.and_then(|id| self.guilds.get(&id));
		let others = self.guilds.values().filter(|guild| Some(guild.id) != guild_id);
		let guilds = current
			.into_iter()
			.chain(others)
			.map(|guild| GuildEmojis {
				guild_id: guild.id,
				name: guild.name.clone(),
				icon: guild.icon.clone(),
				emojis: guild
					.emojis
					.iter()
					.filter_map(|emoji| catalog_emoji(emoji, guild.id, guild_id, nitro, permissions))
					.collect(),
			})
			.filter(|section| !section.emojis.is_empty())
			.collect();
		EmojiCatalog {
			unicode: &UNICODE_SECTIONS,
			guilds,
		}
	}

	pub fn can_use_emoji(&self, emoji: &CatalogEmoji, channel_id: Snowflake) -> bool {
		emoji_usable(
			emoji.guild_id,
			emoji.animated,
			emoji.available,
			self.channel_guild(channel_id),
			self.has_nitro(),
			self.my_permissions(channel_id),
		)
	}

	pub(crate) fn guild_stickers_updated(&mut self, update: &GuildStickersUpdate) -> Changes {
		self.lazy
			.expressions
			.guild_stickers
			.insert(update.guild_id, update.stickers.to_vec());
		let mut changes = Changes::default();
		changes.loaded.insert(update.guild_id);
		changes
	}
}
