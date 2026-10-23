use bitflags::bitflags;
use serde::{Deserialize, Serialize};

use crate::lenient::{flags_serde, int_enum};
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
	pub username: String,
	pub discriminator: Option<String>,
	pub global_name: Option<String>,
	pub avatar: Option<String>,
	pub bot: Option<bool>,
	pub system: Option<bool>,
	pub banner: Option<String>,
	pub accent_color: Option<i64>,
	pub flags: UserFlags,
	pub premium_type: Option<PremiumType>,
	pub public_flags: UserFlags,
	pub avatar_decoration_data: Option<AvatarDecorationData>,
	pub primary_guild: Option<PrimaryGuild>,
	pub clan: Option<PrimaryGuild>,
	pub email: Option<String>,
	pub verified: Option<bool>,
	pub mfa_enabled: Option<bool>,
	pub locale: Option<String>,
}
