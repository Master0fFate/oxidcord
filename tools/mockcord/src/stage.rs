use chrono::{DateTime, Utc};
use protocol::{ChannelType, MessageType, Snowflake, StageInstance, StagePrivacy, VoiceState};
use serde_json::Value;

use crate::error::ApiError;
use crate::fixtures::{CURRENT_USER_ID, GuildFixture};
use crate::json::{parse_id, to_json};
use crate::ops::Draft;
use crate::state::{Shared, Store};

const MAX_TOPIC_CHARS: usize = 120;

#[derive(Clone, Default)]
pub struct Seat {
	pub suppress: bool,
	pub request_to_speak_timestamp: Option<DateTime<Utc>>,
	pub mute: bool,
	pub deaf: bool,
	pub self_mute: bool,
	pub self_deaf: bool,
	pub self_video: bool,
}

impl Seat {
	fn apply(&mut self, body: &Value, own: bool, now: DateTime<Utc>) -> bool {
		let was_speaker = !self.suppress;
		match body["suppress"].as_bool() {
			Some(true) => {
				self.suppress = true;
				self.request_to_speak_timestamp = None;
			}
			Some(false) if own || self.request_to_speak_timestamp.is_some() => {
				self.suppress = false;
				self.request_to_speak_timestamp = None;
			}
			Some(false) => self.request_to_speak_timestamp = Some(now),
			None => {}
		}
		if own && let Some(request) = body.get("request_to_speak_timestamp") {
			self.request_to_speak_timestamp = serde_json::from_value(request.clone()).ok().flatten();
		}
		!was_speaker && !self.suppress
	}

	pub fn write_to(&self, state: &mut VoiceState) {
		state.suppress = self.suppress;
		state.request_to_speak_timestamp = self.request_to_speak_timestamp;
		state.mute = self.mute;
		state.deaf = self.deaf;
	}
}

impl Store {
	pub fn guild_stage_instances(&self, guild_id: u64) -> Vec<&StageInstance> {
		self.stage_instances
			.values()
			.filter(|instance| instance.guild_id.0 == guild_id)
			.collect()
	}

	pub fn guild_voice_states(&self, guild: &GuildFixture) -> Vec<VoiceState> {
		guild
			.voice_states
			.iter()
			.cloned()
			.map(|mut state| {
				if let Some(seat) = self.seats.get(&state.user_id.0) {
					seat.write_to(&mut state);
				}
				state
			})
			.collect()
	}
}

fn topic_of(body: &Value) -> Result<String, ApiError> {
	body["topic"]
		.as_str()
		.map(str::trim)
		.filter(|topic| !topic.is_empty() && topic.chars().count() <= MAX_TOPIC_CHARS)
		.map(str::to_owned)
		.ok_or_else(|| ApiError::invalid_body("topic must be 1-120 characters"))
}

impl Shared {
	pub fn stage_instance(&self, channel_id: u64) -> Result<Value, ApiError> {
		self.store
			.lock()
			.stage_instances
			.get(&channel_id)
			.map(to_json)
			.ok_or_else(ApiError::not_found)
	}

	pub fn create_stage_instance(&self, body: &Value) -> Result<Value, ApiError> {
		let channel_id =
			parse_id(&body["channel_id"]).ok_or_else(|| ApiError::invalid_body("channel_id is required"))?;
		let topic = topic_of(body)?;
		let instance = {
			let mut store = self.store.lock();
			let guild_id = Self::stage_guild(&store, channel_id)?;
			if store.stage_instances.contains_key(&channel_id) {
				return Err(ApiError::invalid_body("Stage instance already exists"));
			}
			let instance = StageInstance {
				id: Snowflake(store.next_snowflake()),
				guild_id: Snowflake(guild_id),
				channel_id: Snowflake(channel_id),
				topic,
				privacy_level: StagePrivacy::GuildOnly,
				guild_scheduled_event_id: None,
			};
			store.stage_instances.insert(channel_id, instance.clone());
			instance
		};
		let value = to_json(&instance);
		self.hub.broadcast("STAGE_INSTANCE_CREATE", &value);
		self.stage_notice(
			channel_id,
			MessageType::StageStart,
			&instance.topic,
			CURRENT_USER_ID,
		)?;
		Ok(value)
	}

	pub fn modify_stage_instance(&self, channel_id: u64, body: &Value) -> Result<Value, ApiError> {
		let topic = topic_of(body)?;
		let instance = {
			let mut store = self.store.lock();
			let instance = store
				.stage_instances
				.get_mut(&channel_id)
				.ok_or_else(ApiError::not_found)?;
			instance.topic.clone_from(&topic);
			instance.clone()
		};
		let value = to_json(&instance);
		self.hub.broadcast("STAGE_INSTANCE_UPDATE", &value);
		self.stage_notice(channel_id, MessageType::StageTopic, &topic, CURRENT_USER_ID)?;
		Ok(value)
	}

	pub fn delete_stage_instance(&self, channel_id: u64) -> Result<(), ApiError> {
		let instance = self
			.store
			.lock()
			.stage_instances
			.remove(&channel_id)
			.ok_or_else(ApiError::not_found)?;
		self.hub.broadcast("STAGE_INSTANCE_DELETE", &to_json(&instance));
		self.stage_notice(
			channel_id,
			MessageType::StageEnd,
			&instance.topic,
			CURRENT_USER_ID,
		)
	}

	pub fn patch_voice_state(&self, guild_id: u64, user: u64, body: &Value) -> Result<(), ApiError> {
		let channel_id =
			parse_id(&body["channel_id"]).ok_or_else(|| ApiError::invalid_body("channel_id is required"))?;
		let own = user == CURRENT_USER_ID;
		let join = self.voice_join.lock().clone();
		let (event, speaker) = {
			let mut store = self.store.lock();
			Self::stage_guild(&store, channel_id)?;
			let fixture = self
				.fixtures
				.guild(guild_id)
				.and_then(|guild| {
					guild.voice_states.iter().find(|state| {
						state.user_id.0 == user && state.channel_id.map(Snowflake::get) == Some(channel_id)
					})
				})
				.cloned();
			let seated = match (&fixture, &join) {
				(Some(_), _) => true,
				(None, Some(join)) => own && join.channel_id == channel_id,
				(None, None) => false,
			};
			if !seated {
				return Err(ApiError::invalid_body("User is not in the stage channel"));
			}
			let start = fixture.as_ref().map_or_else(Seat::default, |state| Seat {
				suppress: state.suppress,
				request_to_speak_timestamp: state.request_to_speak_timestamp,
				..Seat::default()
			});
			let seat = store.seats.entry(user).or_insert(start);
			let speaker = seat.apply(body, own, Utc::now());
			let seat = seat.clone();
			let event = match (fixture, &join) {
				(Some(mut state), _) => {
					seat.write_to(&mut state);
					to_json(&state)
				}
				(None, Some(join)) => {
					drop(store);
					self.voice_state_event(&join.session, join.guild_id, Some(join.channel_id))
				}
				(None, None) => return Err(ApiError::unknown_channel()),
			};
			(event, speaker)
		};
		self.hub.broadcast("VOICE_STATE_UPDATE", &event);
		if speaker {
			self.stage_notice(channel_id, MessageType::StageSpeaker, "", user)?;
		}
		Ok(())
	}

	pub fn sync_seat(&self, rejoined: bool, channel_id: Option<u64>, data: &Value) {
		let mut store = self.store.lock();
		if rejoined {
			let stage = channel_id.is_some_and(|id| Self::stage_guild(&store, id).is_ok());
			let seat = store.seats.entry(CURRENT_USER_ID).or_default();
			seat.suppress = stage;
			seat.request_to_speak_timestamp = None;
		}
		let seat = store.seats.entry(CURRENT_USER_ID).or_default();
		seat.self_mute = data["self_mute"].as_bool().unwrap_or(false);
		seat.self_deaf = data["self_deaf"].as_bool().unwrap_or(false);
		seat.self_video = data["self_video"].as_bool().unwrap_or(false);
	}

	pub fn current_seat(&self) -> Seat {
		self.store
			.lock()
			.seats
			.get(&CURRENT_USER_ID)
			.cloned()
			.unwrap_or_default()
	}

	fn stage_guild(store: &Store, channel_id: u64) -> Result<u64, ApiError> {
		let channel = store
			.channels
			.get(&channel_id)
			.ok_or_else(ApiError::unknown_channel)?;
		match channel.guild_id {
			Some(guild_id) if channel.kind == ChannelType::GuildStageVoice => Ok(guild_id.0),
			_ => Err(ApiError::wrong_channel_kind()),
		}
	}

	fn stage_notice(
		&self,
		channel_id: u64,
		kind: MessageType,
		content: &str,
		author_id: u64,
	) -> Result<(), ApiError> {
		let author = self
			.fixtures
			.users
			.get(&author_id)
			.cloned()
			.unwrap_or_else(|| self.store.lock().current_user.clone());
		let notice = Draft {
			kind: Some(kind),
			content: content.to_owned(),
			..Draft::default()
		};
		self.post_message(channel_id, &author, notice).map(drop)
	}
}
