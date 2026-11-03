use bitflags::bitflags;
use serde::{Deserialize, Serialize};

use crate::lenient::{flags_serde, int_enum, or_default, str_enum};
use crate::snowflake::Snowflake;

bitflags! {
	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
	pub struct UserFlags: u64 {
		const STAFF = 1 << 0;
		const PARTNER = 1 << 1;
		const HYPESQUAD = 1 << 2;
		const BUG_HUNTER_LEVEL_1 = 1 << 3;
		const HYPESQUAD_ONLINE_HOUSE_1 = 1 << 6;
		const HYPESQUAD_ONLINE_HOUSE_2 = 1 << 7;
		const HYPESQUAD_ONLINE_HOUSE_3 = 1 << 8;
		const PREMIUM_EARLY_SUPPORTER = 1 << 9;
		const TEAM_PSEUDO_USER = 1 << 10;
		const BUG_HUNTER_LEVEL_2 = 1 << 14;
		const VERIFIED_BOT = 1 << 16;
		const VERIFIED_DEVELOPER = 1 << 17;
		const CERTIFIED_MODERATOR = 1 << 18;
		const BOT_HTTP_INTERACTIONS = 1 << 19;
		const ACTIVE_DEVELOPER = 1 << 22;
	}
}

flags_serde!(UserFlags, number);

int_enum! {
	pub enum PremiumType {
		None = 0,
		NitroClassic = 1,
		Nitro = 2,
		NitroBasic = 3,
	}
}

str_enum! {
	pub enum NameplatePalette {
		None = "none",
		Crimson = "crimson",
		Berry = "berry",
		Sky = "sky",
		Teal = "teal",
		Forest = "forest",
		BubbleGum = "bubble_gum",
		Violet = "violet",
		Cobalt = "cobalt",
		Clover = "clover",
		Lemon = "lemon",
		White = "white",
	}
}

int_enum! {
	pub enum DisplayNameEffect {
		Solid = 1,
		Gradient = 2,
		Neon = 3,
		Toon = 4,
		Pop = 5,
		Glow = 6,
		Prism = 7,
		Gummy = 8,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Nameplate {
	#[serde(deserialize_with = "or_default")]
	pub asset: String,
	pub sku_id: Option<Snowflake>,
	#[serde(deserialize_with = "or_default")]
	pub label: Option<String>,
	pub palette: NameplatePalette,
	#[serde(deserialize_with = "or_default")]
	pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Collectibles {
	#[serde(deserialize_with = "or_default")]
	pub nameplate: Option<Nameplate>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayNameStyles {
	#[serde(deserialize_with = "or_default")]
	pub font_id: Option<i64>,
	pub effect_id: DisplayNameEffect,
	#[serde(deserialize_with = "or_default")]
	pub colors: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AvatarDecorationData {
	pub asset: Option<String>,
	pub sku_id: Option<Snowflake>,
	pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PrimaryGuild {
	pub identity_guild_id: Option<Snowflake>,
	pub identity_enabled: Option<bool>,
	pub tag: Option<String>,
	pub badge: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
	pub id: Snowflake,
	#[serde(deserialize_with = "or_default")]
	pub username: String,
	#[serde(deserialize_with = "or_default")]
	pub discriminator: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub global_name: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub avatar: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub bot: Option<bool>,
	#[serde(deserialize_with = "or_default")]
	pub system: Option<bool>,
	#[serde(deserialize_with = "or_default")]
	pub banner: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub accent_color: Option<i64>,
	pub flags: UserFlags,
	#[serde(deserialize_with = "or_default")]
	pub premium_type: Option<PremiumType>,
	pub public_flags: UserFlags,
	#[serde(deserialize_with = "or_default")]
	pub avatar_decoration_data: Option<AvatarDecorationData>,
	#[serde(deserialize_with = "or_default")]
	pub primary_guild: Option<PrimaryGuild>,
	#[serde(deserialize_with = "or_default")]
	pub clan: Option<PrimaryGuild>,
	#[serde(deserialize_with = "or_default")]
	pub collectibles: Option<Collectibles>,
	#[serde(deserialize_with = "or_default")]
	pub display_name_styles: Option<DisplayNameStyles>,
	#[serde(deserialize_with = "or_default")]
	pub email: Option<String>,
	#[serde(deserialize_with = "or_default")]
	pub verified: Option<bool>,
	#[serde(deserialize_with = "or_default")]
	pub mfa_enabled: Option<bool>,
	#[serde(deserialize_with = "or_default")]
	pub locale: Option<String>,
}
