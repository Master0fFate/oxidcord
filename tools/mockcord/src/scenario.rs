use std::sync::atomic::Ordering;

use protocol::ChannelType;
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

#[derive(Clone, PartialEq)]
pub struct VoiceJoin {
	pub session: String,
	pub guild_id: Option<u64>,
	pub channel_id: u64,
}

impl Shared {
	pub fn voice_state_event(
		&self,
		session: &str,
		guild_id: Option<u64>,
		channel_id: Option<u64>,
		flags: &Value,
	) -> Value {
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
			"self_deaf": flags["self_deaf"].as_bool().unwrap_or(false),
			"self_mute": flags["self_mute"].as_bool().unwrap_or(false),
			"self_stream": false,
			"self_video": flags["self_video"].as_bool().unwrap_or(false),
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
		let event = self.voice_state_event(
			OTHER_DEVICE_SESSION,
			join.guild_id,
			Some(join.channel_id),
			&Value::Null,
		);
		self.hub.broadcast("VOICE_STATE_UPDATE", &event);
		Ok(())
	}

	pub fn voice_move(&self) -> Result<(), ApiError> {
		let join = self.current_voice()?;
		let target = self.other_voice_channel(&join)?;
		let event = self.voice_state_event(&join.session, join.guild_id, Some(target), &Value::Null);
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
		let event = self.voice_state_event(&join.session, join.guild_id, Some(join.channel_id), &Value::Null);
		self.hub.broadcast("VOICE_STATE_UPDATE", &event);
		Ok(())
	}

	pub fn voice_close(&self, code: u16) {
		if matches!(code, CLOSE_SESSION_INVALID | CLOSE_SESSION_TIMEOUT) {
			self.voice_stale.store(true, Ordering::Release);
		}
		let _ = self.voice_signals.send(Signal::Close(code));
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
