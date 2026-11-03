use serde::{Deserialize, Deserializer, Serialize};

use crate::emoji::PartialEmoji;
use crate::lenient::LenientVec;
use crate::snowflake::Snowflake;
use crate::user::User;

pub const SOUND_MAX_BYTES: usize = 512 * 1024;
pub const SOUND_MAX_SECONDS: f32 = 5.2;
pub const SOUND_NAME_MIN: usize = 2;
pub const SOUND_NAME_MAX: usize = 32;

fn full_volume<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
	Ok(Option::<f32>::deserialize(deserializer)?.unwrap_or(1.0))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundboardSound {
	pub sound_id: Snowflake,
	pub name: String,
	#[serde(deserialize_with = "full_volume")]
	pub volume: f32,
	pub emoji_id: Option<Snowflake>,
	pub emoji_name: Option<String>,
	pub guild_id: Option<Snowflake>,
	pub available: Option<bool>,
	pub user: Option<User>,
	pub user_id: Option<Snowflake>,
}

impl Default for SoundboardSound {
	fn default() -> Self {
		Self {
			sound_id: Snowflake::default(),
			name: String::new(),
			volume: 1.0,
			emoji_id: None,
			emoji_name: None,
			guild_id: None,
			available: None,
			user: None,
			user_id: None,
		}
	}
}

impl SoundboardSound {
	pub fn usable(&self) -> bool {
		self.available != Some(false)
	}

	pub fn creator_id(&self) -> Option<Snowflake> {
		self.user.as_ref().map(|user| user.id).or(self.user_id)
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundboardSounds {
	pub guild_id: Snowflake,
	pub soundboard_sounds: LenientVec<SoundboardSound>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundboardSoundList {
	pub items: LenientVec<SoundboardSound>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundboardSoundDelete {
	pub guild_id: Snowflake,
	pub sound_id: Snowflake,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceChannelEffect {
	pub channel_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub user_id: Snowflake,
	pub emoji: Option<PartialEmoji>,
	pub animation_type: Option<i64>,
	pub animation_id: Option<i64>,
	pub sound_id: Option<Snowflake>,
	pub sound_volume: Option<f32>,
}
