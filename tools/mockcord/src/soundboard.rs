use std::collections::HashMap;

use protocol::{SOUND_MAX_BYTES, SOUND_NAME_MAX, SOUND_NAME_MIN, Snowflake, SoundboardSound};
use rand::Rng;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::{CURRENT_USER_ID, Fixtures};
use crate::ids::snowflake_at;
use crate::json::to_json;
use crate::state::Shared;

const DEFAULT_EPOCH_MS: i64 = 1_644_000_000_000;
const GUILD_EPOCH_MS: i64 = 1_645_000_000_000;
const DEFAULT_SOUNDS: [(&str, &str); 4] = [
	("Quack", "\u{1f986}"),
	("Airhorn", "\u{1f4e2}"),
	("Cricket", "\u{1f997}"),
	("Sad Horn", "\u{1f622}"),
];
const GUILD_SOUNDS: [(&str, &str); 2] = [("Chime", "\u{1f514}"), ("Blip", "\u{1f4a1}")];
const BLIP_NAME: &str = "blip";
const DATA_URI_SEPARATOR: &str = ";base64,";

pub const CHIME_FILE: &str = "sound-chime.ogg";
pub const BLIP_FILE: &str = "sound-blip.ogg";

pub fn default_sounds() -> Vec<SoundboardSound> {
	DEFAULT_SOUNDS
		.iter()
		.enumerate()
		.map(|(index, (name, emoji))| SoundboardSound {
			sound_id: Snowflake(snowflake_at(DEFAULT_EPOCH_MS + index as i64 * 1000, 0)),
			name: (*name).to_owned(),
			emoji_name: Some((*emoji).to_owned()),
			available: Some(true),
			..SoundboardSound::default()
		})
		.collect()
}

pub fn seed_sounds(fixtures: &Fixtures) -> HashMap<u64, Vec<SoundboardSound>> {
	fixtures
		.guilds
		.iter()
		.enumerate()
		.map(|(guild_index, guild)| {
			let sounds = GUILD_SOUNDS
				.iter()
				.enumerate()
				.map(|(index, (name, emoji))| {
					let offset = (guild_index * GUILD_SOUNDS.len() + index) as i64;
					SoundboardSound {
						sound_id: Snowflake(snowflake_at(GUILD_EPOCH_MS + offset * 1000, 0)),
						name: (*name).to_owned(),
						emoji_name: Some((*emoji).to_owned()),
						guild_id: Some(Snowflake(guild.id)),
						available: Some(true),
						user: Some(fixtures.current_user.clone()),
						..SoundboardSound::default()
					}
				})
				.collect();
			(guild.id, sounds)
		})
		.collect()
}

fn decoded_len(data_uri: &str) -> Option<usize> {
	let (_, payload) = data_uri.split_once(DATA_URI_SEPARATOR)?;
	Some(payload.len() * 3 / 4)
}

fn checked_name(body: &Value) -> Result<&str, ApiError> {
	let name = body["name"].as_str().unwrap_or_default().trim();
	if (SOUND_NAME_MIN..=SOUND_NAME_MAX).contains(&name.chars().count()) {
		Ok(name)
	} else {
		Err(ApiError::invalid_body("name must be 2-32 characters"))
	}
}

fn volume_of(body: &Value) -> Option<f32> {
	body["volume"].as_f64().map(|volume| volume.clamp(0., 1.) as f32)
}

impl Shared {
	pub fn find_sound(&self, sound_id: u64) -> Option<SoundboardSound> {
		default_sounds()
			.into_iter()
			.chain(self.store.lock().sounds.values().flatten().cloned())
			.find(|sound| sound.sound_id.0 == sound_id)
	}

	pub fn sound_file(&self, sound_id: u64) -> Option<&'static str> {
		let sound = self.find_sound(sound_id)?;
		Some(if sound.name.eq_ignore_ascii_case(BLIP_NAME) {
			BLIP_FILE
		} else {
			CHIME_FILE
		})
	}

	pub fn soundboard_event(&self, guild_id: u64) -> Option<Value> {
		let store = self.store.lock();
		let sounds = store.sounds.get(&guild_id)?;
		Some(json!({ "guild_id": guild_id.to_string(), "soundboard_sounds": to_json(sounds) }))
	}

	pub fn guild_sounds(&self, guild_id: u64) -> Result<Vec<SoundboardSound>, ApiError> {
		self.store
			.lock()
			.sounds
			.get(&guild_id)
			.cloned()
			.ok_or_else(ApiError::unknown_guild)
	}

	pub fn create_sound(&self, guild_id: u64, body: &Value) -> Result<SoundboardSound, ApiError> {
		let name = checked_name(body)?;
		let uploaded = body["sound"].as_str().and_then(decoded_len);
		if uploaded.is_none_or(|bytes| bytes > SOUND_MAX_BYTES) {
			return Err(ApiError::invalid_body("sound must be an audio file up to 512 KB"));
		}
		let mut store = self.store.lock();
		let id = store.next_snowflake();
		let user = store.current_user.clone();
		let sound = SoundboardSound {
			sound_id: Snowflake(id),
			name: name.to_owned(),
			volume: volume_of(body).unwrap_or(1.),
			emoji_id: None,
			emoji_name: body["emoji_name"].as_str().map(str::to_owned),
			guild_id: Some(Snowflake(guild_id)),
			available: Some(true),
			user: Some(user),
			user_id: None,
		};
		store
			.sounds
			.get_mut(&guild_id)
			.ok_or_else(ApiError::unknown_guild)?
			.push(sound.clone());
		drop(store);
		self.hub
			.broadcast("GUILD_SOUNDBOARD_SOUND_CREATE", &to_json(&sound));
		Ok(sound)
	}

	pub fn edit_sound(
		&self,
		guild_id: u64,
		sound_id: u64,
		body: &Value,
	) -> Result<SoundboardSound, ApiError> {
		let mut store = self.store.lock();
		let sound = store
			.sounds
			.get_mut(&guild_id)
			.ok_or_else(ApiError::unknown_guild)?
			.iter_mut()
			.find(|sound| sound.sound_id.0 == sound_id)
			.ok_or_else(ApiError::not_found)?;
		if body.get("name").is_some() {
			sound.name = checked_name(body)?.to_owned();
		}
		if let Some(volume) = volume_of(body) {
			sound.volume = volume;
		}
		let updated = sound.clone();
		drop(store);
		self.hub
			.broadcast("GUILD_SOUNDBOARD_SOUND_UPDATE", &to_json(&updated));
		Ok(updated)
	}

	pub fn delete_sound(&self, guild_id: u64, sound_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		let sounds = store
			.sounds
			.get_mut(&guild_id)
			.ok_or_else(ApiError::unknown_guild)?;
		let before = sounds.len();
		sounds.retain(|sound| sound.sound_id.0 != sound_id);
		if sounds.len() == before {
			return Err(ApiError::not_found());
		}
		drop(store);
		self.hub.broadcast(
			"GUILD_SOUNDBOARD_SOUND_DELETE",
			&json!({ "guild_id": guild_id.to_string(), "sound_id": sound_id.to_string() }),
		);
		Ok(())
	}

	pub fn send_sound(&self, channel_id: u64, sound_id: u64) -> Result<(), ApiError> {
		let join = self
			.voice_join
			.lock()
			.clone()
			.filter(|join| join.channel_id == channel_id)
			.ok_or_else(|| ApiError::invalid_body("Not in that voice channel"))?;
		let sound = self.find_sound(sound_id).ok_or_else(ApiError::not_found)?;
		self.broadcast_effect(CURRENT_USER_ID, channel_id, join.guild_id, &sound);
		Ok(())
	}

	pub fn broadcast_effect(
		&self,
		user_id: u64,
		channel_id: u64,
		guild_id: Option<u64>,
		sound: &SoundboardSound,
	) {
		self.hub.broadcast(
			"VOICE_CHANNEL_EFFECT_SEND",
			&json!({
				"channel_id": channel_id.to_string(),
				"guild_id": guild_id.map(|id| id.to_string()),
				"user_id": user_id.to_string(),
				"emoji": { "id": null, "name": sound.emoji_name, "animated": false },
				"animation_type": 1,
				"animation_id": 0,
				"sound_id": sound.sound_id.to_string(),
				"sound_volume": sound.volume,
			}),
		);
	}

	pub fn play_fixture_sound(&self) {
		let Some(join) = self.voice_join.lock().clone() else {
			return;
		};
		let Some(guild_id) = join.guild_id else {
			return;
		};
		let Some(guild) = self.fixtures.guild(guild_id) else {
			return;
		};
		let speakers: Vec<u64> = guild
			.voice_states
			.iter()
			.filter(|state| state.channel_id.is_some_and(|id| id.0 == join.channel_id))
			.map(|state| state.user_id.0)
			.filter(|&user_id| user_id != CURRENT_USER_ID)
			.collect();
		let sounds = self.guild_sounds(guild_id).unwrap_or_default();
		if speakers.is_empty() || sounds.is_empty() {
			return;
		}
		let mut rng = rand::rng();
		let speaker = speakers[rng.random_range(0..speakers.len())];
		let sound = &sounds[rng.random_range(0..sounds.len())];
		self.broadcast_effect(speaker, join.channel_id, Some(guild_id), sound);
		tracing::info!(channel_id = join.channel_id, "live soundboard effect");
	}
}
