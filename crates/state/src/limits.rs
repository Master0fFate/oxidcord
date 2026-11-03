use protocol::{PremiumType, Snowflake};

use crate::state::State;

const BASE_UPLOAD_BYTES: u64 = 20 * 1024 * 1024;
pub const PREMIUM_UPLOAD_BYTES: u64 = 500 * 1024 * 1024;
const BASE_MESSAGE_CHARS: usize = 2000;
const NITRO_MESSAGE_CHARS: usize = 4000;
const BOOST_TIER_FOR_LARGE_UPLOADS: u8 = 3;
const EMOJI_SLOTS_BY_TIER: [usize; 4] = [50, 100, 150, 250];
const STICKER_SLOTS_BY_TIER: [usize; 4] = [5, 15, 30, 60];
const MORE_EMOJI_SLOTS: usize = 200;
const MORE_STICKER_SLOTS: usize = 60;
const SOUND_SLOTS_BY_TIER: [usize; 4] = [8, 24, 36, 48];
const MORE_SOUND_SLOTS: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpressionLimits {
	pub emoji: usize,
	pub stickers: usize,
	pub sounds: usize,
}

impl State {
	pub fn upload_limit(&self, channel_id: Snowflake) -> u64 {
		let boosted = self
			.channel_guild(channel_id)
			.and_then(|guild_id| self.guild(guild_id))
			.is_some_and(|guild| guild.premium_tier >= BOOST_TIER_FOR_LARGE_UPLOADS);
		let nitro = self
			.current_user()
			.and_then(|user| user.premium_type)
			.is_some_and(|kind| kind == PremiumType::Nitro);
		if boosted || nitro {
			PREMIUM_UPLOAD_BYTES
		} else {
			BASE_UPLOAD_BYTES
		}
	}

	pub fn expression_limits(&self, guild_id: Snowflake) -> ExpressionLimits {
		let guild = self.guild(guild_id);
		let tier = guild.map_or(0, |guild| {
			usize::from(guild.premium_tier).min(EMOJI_SLOTS_BY_TIER.len() - 1)
		});
		let featured = |feature: &str| guild.is_some_and(|guild| guild.has_feature(feature));
		ExpressionLimits {
			emoji: if featured("MORE_EMOJI") {
				MORE_EMOJI_SLOTS.max(EMOJI_SLOTS_BY_TIER[tier])
			} else {
				EMOJI_SLOTS_BY_TIER[tier]
			},
			stickers: if featured("MORE_STICKERS") {
				MORE_STICKER_SLOTS.max(STICKER_SLOTS_BY_TIER[tier])
			} else {
				STICKER_SLOTS_BY_TIER[tier]
			},
			sounds: if featured("MORE_SOUNDBOARD") {
				MORE_SOUND_SLOTS.max(SOUND_SLOTS_BY_TIER[tier])
			} else {
				SOUND_SLOTS_BY_TIER[tier]
			},
		}
	}

	pub fn message_limit(&self) -> usize {
		let long = self
			.current_user()
			.and_then(|user| user.premium_type)
			.is_some_and(|kind| matches!(kind, PremiumType::Nitro | PremiumType::NitroClassic));
		if long {
			NITRO_MESSAGE_CHARS
		} else {
			BASE_MESSAGE_CHARS
		}
	}
}
