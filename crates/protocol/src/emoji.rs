use serde::{Deserialize, Serialize};

use crate::lenient::int_enum;
use crate::snowflake::Snowflake;
use crate::user::User;

int_enum! {
	pub enum StickerFormat {
		Png = 1,
		Apng = 2,
		Lottie = 3,
		Gif = 4,
	}
}

int_enum! {
	pub enum StickerKind {
		Standard = 1,
		Guild = 2,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialEmoji {
	pub id: Option<Snowflake>,
	pub name: Option<String>,
	pub animated: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Emoji {
	pub id: Option<Snowflake>,
	pub name: Option<String>,
	pub roles: Vec<Snowflake>,
	pub user: Option<User>,
	pub require_colons: Option<bool>,
	pub managed: Option<bool>,
	pub animated: Option<bool>,
	pub available: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sticker {
	pub id: Snowflake,
	pub pack_id: Option<Snowflake>,
	pub name: String,
	pub description: Option<String>,
	pub tags: Option<String>,
	#[serde(rename = "type")]
	pub kind: Option<StickerKind>,
	pub format_type: Option<StickerFormat>,
	pub available: Option<bool>,
	pub guild_id: Option<Snowflake>,
	pub user: Option<User>,
	pub sort_value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StickerItem {
	pub id: Snowflake,
	pub name: String,
	pub format_type: Option<StickerFormat>,
}
