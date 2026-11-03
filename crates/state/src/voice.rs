use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use protocol::{
	Channel, ChannelInfo, LenientVec, PartialEmoji, Permissions, Snowflake, SoundboardSound, StageInstance,
	StreamDelete, StreamState, VoiceChannelEffect, VoiceChannelStartTimeUpdate, VoiceChannelStatusUpdate,
	VoiceState,
};
use serde::Deserialize;
use serde_json::Value;

use crate::changes::Changes;
use crate::state::State;

const VOICE_CUE_BACKLOG: usize = 8;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCue {
	Join,
	Leave,
	Mute,
	Unmute,
	Deafen,
	Undeafen,
	StreamStart,
	StreamEnd,
	CameraOn,
	CameraOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceCueEntry {
	pub id: u64,
	pub cue: VoiceCue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2eeInfo {
	pub epoch: u64,
	pub privacy_code: Option<String>,
	pub members: Vec<Snowflake>,
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
	pub hand_requested: bool,
	pub camera_on: bool,
	pub screen_share: Option<StreamInfo>,
	pub watching: Option<String>,
	pub remote_video: HashMap<Snowflake, VideoKind>,
	pub effects: HashMap<Snowflake, VoiceEffect>,
	pub connected_at: Option<DateTime<Utc>>,
	pub e2ee: Option<E2eeInfo>,
	pub e2ee_failed: bool,
}

pub const VOICE_EFFECT_LIFETIME: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceEffect {
	pub emoji: Option<PartialEmoji>,
	pub sent_at: Instant,
}

impl VoiceEffect {
	pub fn is_live(&self) -> bool {
		self.sent_at.elapsed() < VOICE_EFFECT_LIFETIME
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundBlock {
	NotConnected,
	NoPermission,
	ExternalLocked,
	Silenced,
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
	pub preview_url: Option<String>,
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

	pub fn last_voice_cue_id(&self) -> u64 {
		self.voice_cue_count
	}

	pub fn voice_cues_after(&self, seen: u64) -> impl Iterator<Item = VoiceCue> {
		self.voice_cues
			.iter()
			.filter(move |entry| entry.id > seen)
			.map(|entry| entry.cue)
	}

	fn push_voice_cue(&mut self, cue: VoiceCue) {
		if self.voice_cues.len() == VOICE_CUE_BACKLOG {
			self.voice_cues.pop_front();
		}
		self.voice_cue_count += 1;
		self.voice_cues.push_back(VoiceCueEntry {
			id: self.voice_cue_count,
			cue,
		});
	}

	fn voice_cue_changes(&mut self, cue: VoiceCue) -> Changes {
		self.push_voice_cue(cue);
		Changes {
			voice_cues: true,
			..Changes::default()
		}
	}

	pub fn mic_test(&self) -> Option<MicTest> {
		self.mic_test
	}

	pub fn call(&self, channel_id: Snowflake) -> Option<&Call> {
		self.calls.get(&channel_id)
	}

	pub fn stage_instance(&self, guild_id: Snowflake, channel_id: Snowflake) -> Option<&StageInstance> {
		self.guild(guild_id)?.stage_instances.get(&channel_id)
	}

	pub fn speak_invited(&self) -> bool {
		let (Some(me), Some(session)) = (self.current_user_id(), self.voice()) else {
			return false;
		};
		!session.hand_requested
			&& session
				.guild_id
				.and_then(|guild_id| self.guild(guild_id))
				.and_then(|guild| guild.voice_states.get(&me))
				.is_some_and(|voice| voice.suppress && voice.request_to_speak_timestamp.is_some())
	}

	pub fn server_muted(&self) -> bool {
		self.own_voice_state()
			.is_some_and(|voice| voice.mute || voice.deaf)
	}

	fn own_voice_state(&self) -> Option<&VoiceState> {
		let (me, session) = (self.current_user_id()?, self.voice()?);
		self.guild(session.guild_id?)?.voice_states.get(&me)
	}

	pub fn sound_block(&self, sound: &SoundboardSound) -> Option<SoundBlock> {
		let Some(session) = self
			.voice()
			.filter(|voice| voice.status == VoiceStatus::Connected)
		else {
			return Some(SoundBlock::NotConnected);
		};
		let permissions = self.my_permissions(session.channel_id);
		if !permissions.contains(Permissions::SPEAK | Permissions::USE_SOUNDBOARD) {
			return Some(SoundBlock::NoPermission);
		}
		let external = sound
			.guild_id
			.is_some_and(|owner| Some(owner) != session.guild_id);
		if external && (!self.has_nitro() || !permissions.contains(Permissions::USE_EXTERNAL_SOUNDS)) {
			return Some(SoundBlock::ExternalLocked);
		}
		self.silenced(session).then_some(SoundBlock::Silenced)
	}

	pub fn effect_block(&self) -> Option<SoundBlock> {
		let Some(session) = self
			.voice()
			.filter(|voice| voice.status == VoiceStatus::Connected)
		else {
			return Some(SoundBlock::NotConnected);
		};
		self.silenced(session).then_some(SoundBlock::Silenced)
	}

	fn silenced(&self, session: &VoiceSession) -> bool {
		session.self_deaf
			|| self
				.own_voice_state()
				.is_some_and(|voice| voice.mute || voice.deaf || voice.suppress)
	}

	pub fn calls(&self) -> impl Iterator<Item = &Call> {
		self.calls.values()
	}

	pub fn ringing_recipients(&self, channel_id: Snowflake) -> Vec<Snowflake> {
		let me = self.current_user_id();
		self.call(channel_id).map_or_else(Vec::new, |call| {
			call.ringing
				.iter()
				.copied()
				.filter(|user_id| Some(*user_id) != me)
				.collect()
		})
	}

	pub fn call_has_guests(&self, channel_id: Snowflake) -> bool {
		let me = self.current_user_id();
		self.call(channel_id)
			.is_some_and(|call| call.participants.iter().any(|user_id| Some(*user_id) != me))
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
		let was_connected = self
			.voice
			.as_ref()
			.is_some_and(|session| session.status == VoiceStatus::Connected);
		let first_connect = *status == VoiceStatus::Connected
			&& self.voice.as_ref().is_none_or(|session| {
				session.channel_id != channel_id
					|| session.guild_id != guild_id
					|| session.connected_at.is_none()
			});
		let changed = match (status, self.voice.as_mut()) {
			(VoiceStatus::Disconnected { reason: None }, _) => self.voice.take().is_some(),
			(status, Some(session)) if session.channel_id == channel_id && session.guild_id == guild_id => {
				let mut changed = session.status != *status;
				session.status = status.clone();
				if *status != VoiceStatus::Connected && !session.speaking.is_empty() {
					session.speaking.clear();
					changed = true;
				}
				if *status == VoiceStatus::Connected && session.connected_at.is_none() {
					session.connected_at = Some(Utc::now());
				}
				if matches!(status, VoiceStatus::Disconnected { .. }) {
					session.camera_on = false;
					session.screen_share = None;
					session.watching = None;
					session.remote_video.clear();
					session.effects.clear();
					session.e2ee = None;
					session.e2ee_failed = false;
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
					hand_requested: false,
					camera_on: false,
					screen_share: None,
					watching: None,
					remote_video: HashMap::new(),
					effects: HashMap::new(),
					connected_at: (*status == VoiceStatus::Connected).then(Utc::now),
					e2ee: None,
					e2ee_failed: false,
				});
				true
			}
		};
		let mut changes = voice_session_changes(changed);
		if first_connect {
			changes.merge(self.voice_cue_changes(VoiceCue::Join));
		} else if was_connected && matches!(status, VoiceStatus::Disconnected { .. }) {
			changes.merge(self.voice_cue_changes(VoiceCue::Leave));
		}
		changes
	}

	pub(crate) fn voice_self(&mut self, self_mute: bool, self_deaf: bool) -> Changes {
		let Some(session) = self.voice.as_mut() else {
			return Changes::default();
		};
		let changed = (session.self_mute, session.self_deaf) != (self_mute, self_deaf);
		let cue = if session.status != VoiceStatus::Connected {
			None
		} else if session.self_deaf != self_deaf {
			Some(if self_deaf {
				VoiceCue::Deafen
			} else {
				VoiceCue::Undeafen
			})
		} else if session.self_mute != self_mute {
			Some(if self_mute {
				VoiceCue::Mute
			} else {
				VoiceCue::Unmute
			})
		} else {
			None
		};
		session.self_mute = self_mute;
		session.self_deaf = self_deaf;
		let mut changes = voice_session_changes(changed);
		if let Some(cue) = cue {
			changes.merge(self.voice_cue_changes(cue));
		}
		changes
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

	pub(crate) fn voice_e2ee(&mut self, info: Option<E2eeInfo>) -> Changes {
		self.edit_voice(|session| {
			let changed = session.e2ee != info || session.e2ee_failed;
			session.e2ee = info;
			session.e2ee_failed = false;
			changed
		})
	}

	pub(crate) fn voice_e2ee_failed(&mut self) -> Changes {
		self.edit_voice(|session| !std::mem::replace(&mut session.e2ee_failed, true))
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

	pub(crate) fn stage_instance_changed(&mut self, instance: &StageInstance, live: bool) -> Changes {
		let guild_id = instance.guild_id;
		let Some(guild) = self.guilds.get_mut(&guild_id) else {
			return Changes::default();
		};
		if live {
			guild
				.stage_instances
				.insert(instance.channel_id, instance.clone());
		} else {
			guild.stage_instances.remove(&instance.channel_id);
		}
		let mut changes = Changes::default();
		changes.voice.insert(guild_id);
		changes.channel_lists.insert(guild_id);
		changes
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
		let mut moves = Vec::new();
		for call in self.calls.values_mut() {
			let joined = voice.channel_id == Some(call.channel_id);
			let present = call.participants.contains(&voice.user_id);
			if joined && !present {
				call.participants.push(voice.user_id);
				changed = true;
				moves.push((call.channel_id, VoiceCue::Join));
			} else if !joined && present {
				call.participants.retain(|user_id| *user_id != voice.user_id);
				changed = true;
				moves.push((call.channel_id, VoiceCue::Leave));
			}
		}
		let mut changes = calls_changed_if(changed);
		for (channel_id, cue) in moves {
			changes.merge(self.peer_cue(voice.user_id, channel_id, cue));
		}
		changes
	}

	pub(crate) fn peer_cue(&mut self, user_id: Snowflake, channel_id: Snowflake, cue: VoiceCue) -> Changes {
		let in_channel = self.voice.as_ref().is_some_and(|session| {
			session.channel_id == channel_id && session.status == VoiceStatus::Connected
		});
		if in_channel && self.current_user_id() != Some(user_id) {
			self.voice_cue_changes(cue)
		} else {
			Changes::default()
		}
	}

	pub(crate) fn peer_voice_moved(
		&mut self,
		user_id: Snowflake,
		before: Option<Snowflake>,
		after: Option<Snowflake>,
	) -> Changes {
		if before == after {
			return Changes::default();
		}
		let mut changes = Changes::default();
		if let Some(channel_id) = after {
			changes.merge(self.peer_cue(user_id, channel_id, VoiceCue::Join));
		}
		if let Some(channel_id) = before {
			changes.merge(self.peer_cue(user_id, channel_id, VoiceCue::Leave));
		}
		changes
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
		let preview_url = self
			.streams
			.get(&stream.stream_key)
			.and_then(|known| known.preview_url.clone());
		self.streams.insert(
			stream.stream_key.clone(),
			StreamSummary {
				stream_key: stream.stream_key.clone(),
				user_id: owner_id,
				guild_id,
				channel_id,
				paused: stream.paused,
				viewers: stream.viewer_ids.clone(),
				preview_url,
			},
		);
		let mut voice_changed = false;
		let mut started = false;
		if Some(owner_id) == self.current_user_id()
			&& let Some(session) = self.voice.as_mut()
		{
			let info = StreamInfo {
				stream_key: stream.stream_key.clone(),
				viewers: stream.viewer_ids.clone(),
			};
			voice_changed = session.screen_share.as_ref() != Some(&info);
			started = session.screen_share.is_none();
			session.screen_share = Some(info);
		}
		let mut changes = stream_changes(voice_changed);
		if started {
			changes.merge(self.voice_cue_changes(VoiceCue::StreamStart));
		}
		changes
	}

	pub(crate) fn stream_preview_changed(&mut self, stream_key: &str, url: &str) -> Changes {
		let Some(stream) = self.streams.get_mut(stream_key) else {
			return Changes::default();
		};
		if stream.preview_url.as_deref() == Some(url) {
			return Changes::default();
		}
		stream.preview_url = Some(url.to_owned());
		stream_changes(false)
	}

	pub(crate) fn stream_deleted(&mut self, delete: &StreamDelete) -> Changes {
		let removed = self.streams.remove(&delete.stream_key).is_some();
		let mut voice_changed = false;
		let mut ended = false;
		if let Some(session) = self.voice.as_mut() {
			if session
				.screen_share
				.as_ref()
				.is_some_and(|info| info.stream_key == delete.stream_key)
			{
				session.screen_share = None;
				voice_changed = true;
				ended = true;
			}
			if session.watching.as_deref() == Some(delete.stream_key.as_str()) {
				session.watching = None;
				voice_changed = true;
			}
		}
		let mut changes = if removed || voice_changed {
			stream_changes(voice_changed)
		} else {
			Changes::default()
		};
		if ended {
			changes.merge(self.voice_cue_changes(VoiceCue::StreamEnd));
		}
		changes
	}

	pub(crate) fn hand_requested_changed(&mut self, requested: bool) -> Changes {
		self.edit_voice(|session| std::mem::replace(&mut session.hand_requested, requested) != requested)
	}

	pub(crate) fn camera_changed(&mut self, on: bool) -> Changes {
		let mut changes = self.edit_voice(|session| std::mem::replace(&mut session.camera_on, on) != on);
		let connected = self
			.voice
			.as_ref()
			.is_some_and(|session| session.status == VoiceStatus::Connected);
		if changes.voice_session && connected {
			changes.merge(self.voice_cue_changes(if on {
				VoiceCue::CameraOn
			} else {
				VoiceCue::CameraOff
			}));
		}
		changes
	}

	pub(crate) fn screen_share_changed(&mut self, info: Option<StreamInfo>) -> Changes {
		let starting = info.is_some();
		let started_or_ended = self
			.voice
			.as_ref()
			.is_some_and(|session| session.screen_share.is_some() != starting);
		let mut changes = self.edit_voice(|session| {
			let changed = session.screen_share != info;
			session.screen_share = info;
			changed
		});
		if started_or_ended {
			changes.merge(self.voice_cue_changes(if starting {
				VoiceCue::StreamStart
			} else {
				VoiceCue::StreamEnd
			}));
		}
		changes
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

	pub(crate) fn voice_effect_sent(&mut self, effect: &VoiceChannelEffect) -> Changes {
		self.edit_voice(|session| {
			if session.channel_id != effect.channel_id {
				return false;
			}
			session.effects.retain(|_, shown| shown.is_live());
			session.effects.insert(
				effect.user_id,
				VoiceEffect {
					emoji: effect.emoji.clone(),
					sent_at: Instant::now(),
				},
			);
			true
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

impl State {
	pub(crate) fn voice_channel_status(&mut self, update: &VoiceChannelStatusUpdate) -> Changes {
		self.edit_voice_channel(update.guild_id, update.id, |channel| {
			channel.status.clone_from(&update.status);
		})
	}

	pub(crate) fn voice_channel_start_time(&mut self, update: &VoiceChannelStartTimeUpdate) -> Changes {
		self.edit_voice_channel(update.guild_id, update.id, |channel| {
			channel.voice_start_time = update.voice_start_time;
		})
	}

	pub(crate) fn channel_info(&mut self, info: &ChannelInfo) -> Changes {
		let mut changes = Changes::default();
		for entry in info.channels.iter() {
			changes.merge(self.edit_voice_channel(info.guild_id, entry.id, |channel| {
				channel.status.clone_from(&entry.status);
				channel.voice_start_time = entry.voice_start_time;
			}));
		}
		changes
	}

	fn edit_voice_channel(
		&mut self,
		guild_id: Snowflake,
		channel_id: Snowflake,
		edit: impl FnOnce(&mut Channel),
	) -> Changes {
		let mut changes = Changes::default();
		let Some(guild_id) = Some(guild_id)
			.filter(|id| id.0 != 0)
			.or_else(|| self.channel_guilds.get(&channel_id).copied().flatten())
		else {
			return changes;
		};
		if let Some(channel) = self
			.guilds
			.get_mut(&guild_id)
			.and_then(|guild| guild.channel_mut(channel_id))
		{
			edit(channel);
			changes.channel_lists.insert(guild_id);
			changes.voice.insert(guild_id);
		}
		changes
	}
}

impl State {
	pub fn can_set_voice_status(&self, channel_id: Snowflake) -> bool {
		let permissions = self.my_permissions(channel_id);
		let connected = self.voice.as_ref().is_some_and(|session| {
			session.channel_id == channel_id && session.status == VoiceStatus::Connected
		});
		permissions.contains(Permissions::SET_VOICE_CHANNEL_STATUS)
			&& (connected || permissions.contains(Permissions::MANAGE_CHANNELS))
	}
}
