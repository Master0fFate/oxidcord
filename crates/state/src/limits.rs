use protocol::{PremiumType, Snowflake};

use crate::state::State;

const BASE_UPLOAD_BYTES: u64 = 20 * 1024 * 1024;
pub const PREMIUM_UPLOAD_BYTES: u64 = 500 * 1024 * 1024;
const BASE_MESSAGE_CHARS: usize = 2000;
const NITRO_MESSAGE_CHARS: usize = 4000;
const BOOST_TIER_FOR_LARGE_UPLOADS: u8 = 3;

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
