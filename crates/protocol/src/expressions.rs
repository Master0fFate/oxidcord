use serde::{Deserialize, Serialize};

use crate::emoji::Sticker;
use crate::lenient::{LenientVec, str_enum};
use crate::snowflake::Snowflake;

pub mod unicode;
mod unicode_table;

pub use unicode::{EmojiCategory, UnicodeEmoji, unicode_emoji_list};

str_enum! {
	pub enum GifFormat {
		Mp4 = "mp4",
		Gif = "gif",
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "RawGif")]
pub struct Gif {
	pub id: String,
	pub title: String,
	pub url: String,
	pub src: String,
	pub gif_src: String,
	pub width: u32,
	pub height: u32,
	pub preview: String,
	pub format: GifFormat,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawGif {
	id: String,
	title: String,
	url: String,
	src: String,
	gif_src: String,
	width: u32,
	height: u32,
	preview: String,
	format: Option<GifFormat>,
}

fn format_of(src: &str) -> GifFormat {
	let path = src.split(['?', '#']).next().unwrap_or(src);
	match path.rsplit_once('.') {
		Some((_, extension)) if extension.eq_ignore_ascii_case("mp4") => GifFormat::Mp4,
		Some((_, extension)) if extension.eq_ignore_ascii_case("gif") => GifFormat::Gif,
		Some((_, extension)) => GifFormat::Unknown(extension.to_ascii_lowercase()),
		None => GifFormat::Unknown(String::new()),
	}
}

impl From<RawGif> for Gif {
	fn from(raw: RawGif) -> Self {
		let format = raw.format.unwrap_or_else(|| format_of(&raw.src));
		Self {
			id: raw.id,
			title: raw.title,
			url: raw.url,
			src: raw.src,
			gif_src: raw.gif_src,
			width: raw.width,
			height: raw.height,
			preview: raw.preview,
			format,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GifCategory {
	pub name: String,
	pub src: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GifTrending {
	pub categories: LenientVec<GifCategory>,
	pub gifs: LenientVec<Gif>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GifFavorites {
	pub gifs: Vec<Gif>,
	pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StickerPack {
	pub id: Snowflake,
	pub name: String,
	pub sku_id: Snowflake,
	pub cover_sticker_id: Option<Snowflake>,
	pub description: Option<String>,
	pub banner_asset_id: Option<Snowflake>,
	pub stickers: LenientVec<Sticker>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StickerPacks {
	pub sticker_packs: LenientVec<StickerPack>,
}

pub type GuildStickers = LenientVec<Sticker>;
