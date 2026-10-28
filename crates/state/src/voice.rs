use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use protocol::{LenientVec, Snowflake, StreamDelete, StreamState, VoiceState};
use serde::Deserialize;
use serde_json::Value;

use crate::changes::Changes;
use crate::state::State;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceStatus {
	Connecting,
	Connected,
	Reconnecting,
	Disconnected { reason: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VoiceStats {
	pub ping_ms: Option<u32>,
	pub loss: u64,
	pub jitter_ms: f32,
	pub e2ee: bool,
	pub video_bitrate_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VoiceLevels {
	pub input: f32,
	pub output: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MicTest {
	pub level: f32,
	pub remaining_secs: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSession {
	pub channel_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub status: VoiceStatus,
	pub speaking: HashSet<Snowflake>,
	pub stats: VoiceStats,
	pub levels: VoiceLevels,
	pub self_mute: bool,
	pub self_deaf: bool,
	pub camera_on: bool,
	pub screen_share: Option<StreamInfo>,
	pub watching: Option<String>,
	pub remote_video: HashMap<Snowflake, VideoKind>,
	pub connected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoKind {
	Camera,
	Screen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
	pub stream_key: String,
	pub viewers: Vec<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSummary {
	pub stream_key: String,
	pub user_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub channel_id: Snowflake,
	pub paused: bool,
	pub viewers: Vec<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Call {
	pub channel_id: Snowflake,
	pub ringing: Vec<Snowflake>,
	pub participants: Vec<Snowflake>,
	pub region: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CallPayload {
	channel_id: Snowflake,
	ringing: LenientVec<Snowflake>,
	region: Option<String>,
	voice_states: Option<LenientVec<VoiceState>>,
}

impl State {
	pub fn voice(&self) -> Option<&VoiceSession> {
		self.voice.as_ref()
	}

	pub fn mic_test(&self) -> Option<MicTest> {
		self.mic_test
	}

	pub fn call(&self, channel_id: Snowflake) -> Option<&Call> {
		self.calls.get(&channel_id)
	}

	pub fn calls(&self) -> impl Iterator<Item = &Call> {
		self.calls.values()
	}

	pub fn incoming_calls(&self) -> impl Iterator<Item = &Call> {
		let me = self.current_user_id();
		self.calls
			.values()
			.filter(move |call| me.is_some_and(|me| call.ringing.contains(&me)))
	}

	pub(crate) fn voice_connection(
		&mut self,
		status: &VoiceStatus,
		channel_id: Snowflake,
		guild_id: Option<Snowflake>,
	) -> Changes {
		let changed = match (status, self.voice.as_mut()) {
			(VoiceStatus::Disconnected { reason: None }, _) => self.voice.take().is_some(),
			(status, Some(session)) if session.channel_id == channel_id && session.guild_id == guild_id => {
				let changed = session.status != *status;
				session.status = status.clone();
				if *status == VoiceStatus::Connected && session.connected_at.is_none() {
					session.connected_at = Some(Utc::now());
				}
				if matches!(status, VoiceStatus::Disconnected { .. }) {
					session.speaking.clear();
					session.camera_on = false;
					session.screen_share = None;
					session.watching = None;
					session.remote_video.clear();
				}
				changed
			}
			(status, _) => {
				self.voice = Some(VoiceSession {
					channel_id,
					guild_id,
					status: status.clone(),
					speaking: HashSet::new(),
					stats: VoiceStats::default(),
					levels: VoiceLevels::default(),
					self_mute: false,
					self_deaf: false,
					camera_on: false,
					screen_share: None,
					watching: None,
					remote_video: HashMap::new(),
					connected_at: (*status == VoiceStatus::Connected).then(Utc::now),
				});
				true
			}
		};
		voice_session_changes(changed)
	}

	pub(crate) fn voice_self(&mut self, self_mute: bool, self_deaf: bool) -> Changes {
		let changed = self
			.voice
			.as_mut()
			.is_some_and(|session| (session.self_mute, session.self_deaf) != (self_mute, self_deaf));
		if let Some(session) = self.voice.as_mut() {
			session.self_mute = self_mute;
			session.self_deaf = self_deaf;
		}
		voice_session_changes(changed)
	}

	pub(crate) fn voice_speaking(&mut self, user_id: Snowflake, speaking: bool) -> Changes {
		let changed = self.voice.as_mut().is_some_and(|session| {
			if speaking {
				session.speaking.insert(user_id)
			} else {
				session.speaking.remove(&user_id)
			}
		});
		voice_session_changes(changed)
	}

	pub(crate) fn voice_stats(&mut self, stats: VoiceStats) -> Changes {
		let changed = self.voice.as_mut().is_some_and(|session| {
			let changed = session.stats != stats;
			session.stats = stats;
			changed
		});
		voice_session_changes(changed)
	}

	pub(crate) fn mic_test_changed(&mut self, test: Option<MicTest>) -> Changes {
		let changed = std::mem::replace(&mut self.mic_test, test) != test;
		voice_session_changes(changed)
	}

	pub(crate) fn voice_levels(&mut self, levels: VoiceLevels) -> Changes {
		let changed = self.voice.as_mut().is_some_and(|session| {
			let changed = session.levels != levels;
			session.levels = levels;
			changed
		});
		voice_session_changes(changed)
	}

	pub(crate) fn call_changed(&mut self, payload: &Value) -> Changes {
		let Ok(payload) = CallPayload::deserialize(payload) else {
			return Changes::default();
		};
		let call = self.calls.entry(payload.channel_id).or_insert_with(|| Call {
			channel_id: payload.channel_id,
			..Call::default()
		});
		call.ringing = payload.ringing.into_vec();
		if payload.region.is_some() {
			call.region = payload.region;
		}
		if let Some(voice_states) = payload.voice_states {
			call.participants = voice_states
				.into_vec()
				.into_iter()
				.map(|voice| voice.user_id)
				.collect();
		}
		calls_changed()
	}

	pub(crate) fn call_deleted(&mut self, payload: &Value) -> Changes {
		let Ok(payload) = CallPayload::deserialize(payload) else {
			return Changes::default();
		};
		calls_changed_if(self.calls.remove(&payload.channel_id).is_some())
	}

	pub(crate) fn call_voice_state(&mut self, voice: &VoiceState) -> Changes {
		let mut changed = false;
		for call in self.calls.values_mut() {
			let joined = voice.channel_id == Some(call.channel_id);
			let present = call.participants.contains(&voice.user_id);
			if joined && !present {
				call.participants.push(voice.user_id);
				changed = true;
			} else if !joined && present {
				call.participants.retain(|user_id| *user_id != voice.user_id);
				changed = true;
			}
		}
		calls_changed_if(changed)
	}
}

fn voice_session_changes(changed: bool) -> Changes {
	Changes {
		voice_session: changed,
		..Changes::default()
	}
}

fn calls_changed() -> Changes {
	calls_changed_if(true)
}

fn calls_changed_if(changed: bool) -> Changes {
	Changes {
		calls: changed,
		..Changes::default()
	}
}

fn parse_stream_key(key: &str) -> Option<(Option<Snowflake>, Snowflake, Snowflake)> {
	let mut parts = key.split(':');
	let kind = parts.next()?;
	let mut ids = parts.map(|part| part.parse::<u64>().ok().map(Snowflake));
	match kind {
		"guild" => Some((Some(ids.next()??), ids.next()??, ids.next()??)),
		"call" => Some((None, ids.next()??, ids.next()??)),
		_ => None,
	}
}

impl State {
	pub fn streams(&self) -> impl Iterator<Item = &StreamSummary> {
		self.streams.values()
	}

	pub fn stream_of(&self, user_id: Snowflake) -> Option<&StreamSummary> {
		self.streams.values().find(|stream| stream.user_id == user_id)
	}

	pub(crate) fn stream_changed(&mut self, stream: &StreamState) -> Changes {
		let Some((guild_id, channel_id, owner_id)) = parse_stream_key(&stream.stream_key) else {
			return Changes::default();
		};
		self.streams.insert(
			stream.stream_key.clone(),
			StreamSummary {
				stream_key: stream.stream_key.clone(),
				user_id: owner_id,
				guild_id,
				channel_id,
				paused: stream.paused,
				viewers: stream.viewer_ids.clone(),
			},
		);
		let mut voice_changed = false;
		if Some(owner_id) == self.current_user_id()
			&& let Some(session) = self.voice.as_mut()
		{
			let info = StreamInfo {
				stream_key: stream.stream_key.clone(),
				viewers: stream.viewer_ids.clone(),
			};
			voice_changed = session.screen_share.as_ref() != Some(&info);
			session.screen_share = Some(info);
		}
		stream_changes(voice_changed)
	}

	pub(crate) fn stream_deleted(&mut self, delete: &StreamDelete) -> Changes {
		let removed = self.streams.remove(&delete.stream_key).is_some();
		let mut voice_changed = false;
		if let Some(session) = self.voice.as_mut() {
			if session
				.screen_share
				.as_ref()
				.is_some_and(|info| info.stream_key == delete.stream_key)
			{
				session.screen_share = None;
				voice_changed = true;
			}
			if session.watching.as_deref() == Some(delete.stream_key.as_str()) {
				session.watching = None;
				voice_changed = true;
			}
		}
		if removed || voice_changed {
			stream_changes(voice_changed)
		} else {
			Changes::default()
		}
	}

	pub(crate) fn camera_changed(&mut self, on: bool) -> Changes {
		self.edit_voice(|session| std::mem::replace(&mut session.camera_on, on) != on)
	}

	pub(crate) fn screen_share_changed(&mut self, info: Option<StreamInfo>) -> Changes {
		self.edit_voice(|session| {
			let changed = session.screen_share != info;
			session.screen_share = info;
			changed
		})
	}

	pub(crate) fn watching_changed(&mut self, stream_key: Option<String>) -> Changes {
		self.edit_voice(|session| {
			let changed = session.watching != stream_key;
			session.watching = stream_key;
			changed
		})
	}

	pub(crate) fn remote_video_changed(
		&mut self,
		user_id: Snowflake,
		kind: VideoKind,
		active: bool,
	) -> Changes {
		self.edit_voice(|session| {
			if active {
				session.remote_video.insert(user_id, kind) != Some(kind)
			} else {
				session.remote_video.get(&user_id) == Some(&kind)
					&& session.remote_video.remove(&user_id).is_some()
			}
		})
	}

	fn edit_voice(&mut self, edit: impl FnOnce(&mut VoiceSession) -> bool) -> Changes {
		let changed = self.voice.as_mut().is_some_and(edit);
		voice_session_changes(changed)
	}
}

fn stream_changes(voice_changed: bool) -> Changes {
	Changes {
		streams: true,
		voice_session: voice_changed,
		..Changes::default()
	}
}
