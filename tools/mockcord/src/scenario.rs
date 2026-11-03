use std::sync::atomic::Ordering;

use protocol::{ChannelType, Snowflake, VoiceState};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::json::to_json;
use crate::state::Shared;
use crate::voice::{Signal, VOICE_TOKEN};

const OTHER_DEVICE_SESSION: &str = "mock-other-device";
const CLOSE_DISCONNECTED: u16 = 4014;
const CLOSE_SESSION_INVALID: u16 = 4006;
const CLOSE_SESSION_TIMEOUT: u16 = 4009;
const PREVIEW_DATA_PREFIX: &str = "data:image/jpeg;base64,";
const PREVIEW_CDN: &str = "https://cdn.discordapp.com/stream-previews";
const DAVE_EXTERNAL_SENDER_OPCODE: u8 = 25;

#[derive(Clone, PartialEq)]
pub struct VoiceJoin {
	pub session: String,
	pub guild_id: Option<u64>,
	pub channel_id: u64,
}

pub fn stream_create_event(key: &str, channel_id: u64, viewers: &[u64]) -> Value {
	json!({
		"stream_key": key,
		"rtc_server_id": (channel_id + 1).to_string(),
		"rtc_channel_id": channel_id.to_string(),
		"region": "mock",
		"viewer_ids": viewers.iter().map(u64::to_string).collect::<Vec<_>>(),
		"paused": false,
	})
}

impl Shared {
	pub fn occupant_stream(&self, guild_id: Option<u64>, channel_id: u64) -> Option<Value> {
		let guild = self.fixtures.guild(guild_id?)?;
		if guild
			.stage_instance
			.as_ref()
			.is_some_and(|stage| stage.channel_id.0 == channel_id)
		{
			return None;
		}
		let owner = self.stream_occupant(guild_id, channel_id)?;
		let key = format!("guild:{}:{channel_id}:{}", guild.id, owner.user_id.0);
		Some(stream_create_event(&key, channel_id, &[]))
	}

	pub fn voice_state_event(&self, session: &str, guild_id: Option<u64>, channel_id: Option<u64>) -> Value {
		let member = guild_id
			.and_then(|id| self.fixtures.guild(id))
			.map_or(Value::Null, |guild| to_json(guild.current_member()));
		let seat = self.current_seat();
		json!({
			"guild_id": guild_id.map(|id| id.to_string()),
			"channel_id": channel_id.map(|id| id.to_string()),
			"user_id": CURRENT_USER_ID.to_string(),
			"member": member,
			"session_id": session,
			"deaf": seat.deaf,
			"mute": seat.mute,
			"self_deaf": seat.self_deaf,
			"self_mute": seat.self_mute,
			"self_stream": false,
			"self_video": seat.self_video,
			"suppress": seat.suppress,
			"request_to_speak_timestamp": seat.request_to_speak_timestamp,
		})
	}

	pub fn voice_server_event(&self, guild_id: Option<u64>, channel_id: u64) -> Value {
		let mut server = json!({
			"token": VOICE_TOKEN,
			"guild_id": guild_id.map(|id| id.to_string()),
			"endpoint": self.urls.voice,
		});
		if guild_id.is_none() {
			server["channel_id"] = json!(channel_id.to_string());
		}
		server
	}

	pub fn remember_voice(&self, session: &str, guild_id: Option<u64>, channel_id: Option<u64>) -> bool {
		let next = channel_id.map(|channel_id| VoiceJoin {
			session: session.to_owned(),
			guild_id,
			channel_id,
		});
		let previous = std::mem::replace(&mut *self.voice_join.lock(), next.clone());
		previous != next
	}

	fn current_voice(&self) -> Result<VoiceJoin, ApiError> {
		self.voice_join
			.lock()
			.clone()
			.ok_or(ApiError::invalid_body("Not in a voice channel"))
	}

	pub fn voice_elsewhere(&self) -> Result<(), ApiError> {
		let join = self.current_voice()?;
		let event = self.voice_state_event(OTHER_DEVICE_SESSION, join.guild_id, Some(join.channel_id));
		self.hub.broadcast("VOICE_STATE_UPDATE", &event);
		Ok(())
	}

	pub fn voice_move(&self) -> Result<(), ApiError> {
		let join = self.current_voice()?;
		let target = self.other_voice_channel(&join)?;
		let event = self.voice_state_event(&join.session, join.guild_id, Some(target));
		self.hub.broadcast("VOICE_STATE_UPDATE", &event);
		self.remember_voice(&join.session, join.guild_id, Some(target));
		self.voice_close(CLOSE_DISCONNECTED);
		Ok(())
	}

	pub fn voice_reassign(&self) -> Result<(), ApiError> {
		let join = self.current_voice()?;
		let server = self.voice_server_event(join.guild_id, join.channel_id);
		self.hub
			.dispatch_to(&join.session, "VOICE_SERVER_UPDATE", &server);
		Ok(())
	}

	pub fn set_server_voice(&self, mute: bool, deaf: bool) -> Result<(), ApiError> {
		let join = self.current_voice()?;
		{
			let mut store = self.store.lock();
			let seat = store.seats.entry(CURRENT_USER_ID).or_default();
			seat.mute = mute;
			seat.deaf = deaf;
		}
		let event = self.voice_state_event(&join.session, join.guild_id, Some(join.channel_id));
		self.hub.broadcast("VOICE_STATE_UPDATE", &event);
		Ok(())
	}

	pub fn stream_occupant(&self, guild_id: Option<u64>, channel_id: u64) -> Option<VoiceState> {
		self.fixtures
			.guild(guild_id?)?
			.voice_states
			.iter()
			.find(|state| {
				state.channel_id.map(Snowflake::get) == Some(channel_id) && state.user_id.0 != CURRENT_USER_ID
			})
			.cloned()
	}

	pub fn start_stream(&self) -> Result<(), ApiError> {
		let join = self.current_voice()?;
		let mut streamer = self
			.stream_occupant(join.guild_id, join.channel_id)
			.ok_or(ApiError::invalid_body("Nobody in the channel to stream"))?;
		let key = format!(
			"guild:{}:{}:{}",
			join.guild_id.unwrap_or_default(),
			join.channel_id,
			streamer.user_id
		);
		streamer.self_stream = Some(true);
		self.stream_previews.lock().insert(key.clone());
		self.hub.broadcast("VOICE_STATE_UPDATE", &to_json(&streamer));
		self.hub
			.broadcast("STREAM_CREATE", &stream_create_event(&key, join.channel_id, &[]));
		Ok(())
	}

	pub fn store_stream_preview(&self, key: &str, body: &Value) -> Result<(), ApiError> {
		let thumbnail = body["thumbnail"].as_str().unwrap_or_default();
		if !thumbnail.starts_with(PREVIEW_DATA_PREFIX) {
			return Err(ApiError::invalid_body("Preview must be a jpeg data uri"));
		}
		self.stream_previews.lock().insert(key.to_owned());
		Ok(())
	}

	pub fn stream_preview(&self, key: &str) -> Result<Value, ApiError> {
		if !self.stream_previews.lock().contains(key) {
			return Err(ApiError::not_found());
		}
		Ok(json!({ "url": format!("{PREVIEW_CDN}/{}.png", key.replace(':', "-")) }))
	}

	pub fn voice_close(&self, code: u16) {
		if matches!(code, CLOSE_SESSION_INVALID | CLOSE_SESSION_TIMEOUT) {
			self.voice_stale.store(true, Ordering::Release);
		}
		let _ = self.voice_signals.send(Signal::Close(code));
	}

	pub fn voice_dave_failure(&self) -> Result<(), ApiError> {
		self.current_voice()?;
		let frame = vec![0, 0, DAVE_EXTERNAL_SENDER_OPCODE, 0xff, 0xff];
		let _ = self.voice_signals.send(Signal::Binary(frame));
		Ok(())
	}

	fn other_voice_channel(&self, join: &VoiceJoin) -> Result<u64, ApiError> {
		let guild = join
			.guild_id
			.and_then(|id| self.fixtures.guild(id))
			.ok_or(ApiError::invalid_body("Moves need a guild voice channel"))?;
		let store = self.store.lock();
		guild
			.channel_ids
			.iter()
			.copied()
			.find(|id| {
				*id != join.channel_id
					&& store
						.channels
						.get(id)
						.is_some_and(|channel| channel.kind == ChannelType::GuildVoice)
			})
			.ok_or_else(ApiError::unknown_channel)
	}
}
