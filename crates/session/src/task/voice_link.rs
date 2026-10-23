use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use gateway::SendError;
use parking_lot::Mutex;
use protocol::{Snowflake, Token, VoiceServerUpdate, VoiceState};
use state::{LocalEvent, VoiceStatus};
use store::VoiceSettings;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval, sleep};
use voice::video::VideoQuality;
use voice::{InputMode, VoiceConnection, VoiceError, VoiceEvent, VoiceOptions, VoiceParams};

use super::{Core, Internal};
use crate::config::VoiceIo;

const JOIN_TIMEOUT: Duration = Duration::from_secs(15);
const LEVEL_INTERVAL: Duration = Duration::from_millis(50);
const LEVEL_FLOOR: f32 = 0.005;
const FULL_VOLUME_PERCENT: f32 = 100.0;
const AUTO_VAD_THRESHOLD_DB: f32 = -50.0;
const CLOSE_CHANNEL_GONE: u16 = 4014;

pub(crate) type SharedVoice = Arc<Mutex<Option<Arc<VoiceConnection>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Target {
	pub(super) guild_id: Option<Snowflake>,
	pub(super) channel_id: Snowflake,
}

struct Server {
	token: Token,
	endpoint: String,
}

pub(crate) struct VoiceLink {
	io: VoiceIo,
	shared: SharedVoice,
	target: Option<Target>,
	session_id: Option<String>,
	server: Option<Server>,
	pub(super) connection: Option<Arc<VoiceConnection>>,
	pump: Option<JoinHandle<()>>,
	connecting: bool,
	pub(super) generation: u64,
	pub(super) camera: bool,
	self_mute: bool,
	self_deaf: bool,
	transmitting: bool,
	user_volumes: HashMap<Snowflake, f32>,
	default_video_quality: Option<VideoQuality>,
}

impl VoiceLink {
	pub(crate) fn new(io: VoiceIo, shared: SharedVoice) -> Self {
		Self {
			io,
			shared,
			target: None,
			session_id: None,
			server: None,
			connection: None,
			pump: None,
			connecting: false,
			generation: 0,
			camera: false,
			self_mute: false,
			self_deaf: false,
			transmitting: false,
			user_volumes: HashMap::new(),
			default_video_quality: None,
		}
	}
}

impl VoiceLink {
	pub(super) fn target(&self) -> Option<Target> {
		self.target
	}
}

fn settled(level: f32) -> f32 {
	if level < LEVEL_FLOOR { 0.0 } else { level }
}

fn percent(volume: f32) -> u8 {
	(volume * FULL_VOLUME_PERCENT)
		.round()
		.clamp(0.0, f32::from(u8::MAX)) as u8
}

fn vad_threshold_db(auto: bool, threshold_db: f32) -> f32 {
	if auto { AUTO_VAD_THRESHOLD_DB } else { threshold_db }
}

fn input_mode(push_to_talk: bool) -> InputMode {
	if push_to_talk {
		InputMode::PushToTalk
	} else {
		InputMode::VoiceActivity
	}
}

fn close_reason(code: Option<u16>) -> String {
	match code {
		Some(CLOSE_CHANNEL_GONE) => "removed from the voice channel".to_owned(),
		Some(code) => format!("voice connection closed ({code})"),
		None => "voice connection lost".to_owned(),
	}
}

impl Core {
	pub(super) fn join_voice(&mut self, guild_id: Option<Snowflake>, channel_id: Snowflake) {
		let target = Target { guild_id, channel_id };
		if self.voice.target == Some(target) {
			return;
		}
		self.drop_connection();
		self.voice.target = Some(target);
		self.voice.session_id = None;
		self.voice.server = None;
		self.announce(VoiceStatus::Connecting);
		self.apply_local(LocalEvent::VoiceSelf {
			self_mute: self.voice.self_mute,
			self_deaf: self.voice.self_deaf,
		});
		if let Err(error) = self.send_voice_state(guild_id, Some(channel_id)) {
			self.end_voice(Some(error.to_string()), false);
			return;
		}
		let generation = self.voice.generation;
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			sleep(JOIN_TIMEOUT).await;
			let _ = internal.send(Internal::VoiceJoinTimeout(generation));
		});
	}

	pub(super) fn leave_voice(&mut self) {
		self.end_voice(None, true);
	}

	pub(super) fn set_self_mute(&mut self, muted: bool) {
		self.voice.self_mute = muted;
		if let Some(connection) = &self.voice.connection {
			connection.set_mute(muted);
		}
		self.publish_self_state();
	}

	pub(super) fn set_self_deaf(&mut self, deafened: bool) {
		self.voice.self_deaf = deafened;
		if let Some(connection) = &self.voice.connection {
			connection.set_deaf(deafened);
		}
		self.publish_self_state();
	}

	pub(super) fn set_voice_devices(&mut self, input: Option<String>, output: Option<String>) {
		if let Some(connection) = &self.voice.connection {
			connection.set_input_device(input.clone());
			connection.set_output_device(output.clone());
		}
		self.update_voice_settings(move |settings| {
			settings.input_device = input;
			settings.output_device = output;
		});
	}

	pub(super) fn set_user_volume(&mut self, user_id: Snowflake, volume: f32) {
		self.voice.user_volumes.insert(user_id, volume);
		if let Some(connection) = &self.voice.connection {
			connection.set_user_volume(user_id, volume);
		}
	}

	pub(super) fn set_input_mode(&mut self, mode: InputMode) {
		if let Some(connection) = &self.voice.connection {
			connection.set_input_mode(mode);
		}
		self.update_voice_settings(move |settings| settings.push_to_talk = mode == InputMode::PushToTalk);
	}

	pub(super) fn set_push_to_talk(&mut self, transmitting: bool) {
		self.voice.transmitting = transmitting;
		if let Some(connection) = &self.voice.connection {
			connection.set_transmitting(transmitting);
		}
	}

	pub(super) fn set_input_volume(&mut self, volume: f32) {
		if let Some(connection) = &self.voice.connection {
			connection.set_input_volume(volume);
		}
		self.update_voice_settings(move |settings| settings.input_volume = percent(volume));
	}

	pub(super) fn set_output_volume(&mut self, volume: f32) {
		if let Some(connection) = &self.voice.connection {
			connection.set_output_volume(volume);
		}
		self.update_voice_settings(move |settings| settings.output_volume = percent(volume));
	}

	pub(super) fn set_noise_suppression(&mut self, enabled: bool) {
		if let Some(connection) = &self.voice.connection {
			connection.set_noise_suppression(enabled);
		}
		self.update_voice_settings(move |settings| settings.noise_suppression = enabled);
	}

	pub(super) fn set_vad_threshold(&mut self, auto: bool, threshold_db: f32) {
		if let Some(connection) = &self.voice.connection {
			connection.set_vad_threshold_db(vad_threshold_db(auto, threshold_db));
		}
		self.update_voice_settings(move |settings| {
			settings.auto_sensitivity = auto;
			settings.sensitivity_db =
				threshold_db.round().clamp(f32::from(i8::MIN), f32::from(i8::MAX)) as i8;
		});
	}

	pub(super) fn set_default_video_quality(&mut self, quality: VideoQuality) {
		self.voice.default_video_quality = Some(quality);
		self.set_video_quality(quality);
	}

	pub(super) fn request_to_speak(&self, guild_id: Snowflake, channel_id: Snowflake, request: bool) {
		if let Err(error) = self.gateway.voice_state_request_to_speak(
			Some(guild_id),
			Some(channel_id),
			self.voice.self_mute,
			self.voice.self_deaf,
			request.then(Utc::now),
		) {
			tracing::warn!(%error, "request to speak was not sent");
		}
	}

	pub(super) fn start_call(&self, channel_id: Snowflake) {
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			if let Err(error) = rest.ring_call(channel_id, None).await {
				tracing::warn!(%error, "call was not rung");
			}
			let _ = internal.send(Internal::JoinVoice {
				guild_id: None,
				channel_id,
			});
		});
	}

	pub(super) fn decline_call(&self, channel_id: Snowflake) {
		self.spawn_rest("decline call", move |rest| async move {
			rest.stop_ringing(channel_id, None).await
		});
	}

	pub(super) fn resend_voice_state(&self) {
		let Some(target) = self.voice.target else {
			return;
		};
		if let Err(error) = self.send_voice_state(target.guild_id, Some(target.channel_id)) {
			tracing::warn!(%error, "voice state was not sent");
		}
	}

	pub(super) fn voice_state_update(&mut self, update: &VoiceState) {
		let Some(target) = self.voice.target else {
			return;
		};
		if update.guild_id != target.guild_id || self.state.read().current_user_id() != Some(update.user_id) {
			return;
		}
		match update.channel_id {
			Some(channel_id) => {
				if channel_id != target.channel_id {
					self.voice.target = Some(Target { channel_id, ..target });
					let status = self
						.state
						.read()
						.voice()
						.map_or(VoiceStatus::Connecting, |session| session.status.clone());
					self.announce(status);
				}
				if let Some(session_id) = &update.session_id {
					self.voice.session_id = Some(session_id.clone());
				}
				self.try_connect();
			}
			None if self.voice.session_id.is_some() => {
				self.end_voice(Some("disconnected from the channel".to_owned()), false);
			}
			None => {}
		}
	}

	pub(super) fn voice_server_update(&mut self, update: VoiceServerUpdate) {
		let Some(target) = self.voice.target else {
			return;
		};
		let for_target = match target.guild_id {
			Some(guild_id) => update.guild_id == Some(guild_id),
			None => update.guild_id.is_none() && update.channel_id.is_none_or(|id| id == target.channel_id),
		};
		let (true, Some(token), Some(endpoint)) = (for_target, update.token, update.endpoint) else {
			return;
		};
		if self.voice.connection.is_some() || self.voice.connecting {
			self.drop_connection();
			self.announce(VoiceStatus::Connecting);
		}
		self.voice.server = Some(Server { token, endpoint });
		self.try_connect();
	}

	pub(super) fn on_voice_join_timeout(&mut self, generation: u64) {
		if generation == self.voice.generation && self.voice.connection.is_none() && !self.voice.connecting {
			self.end_voice(Some("voice server did not respond".to_owned()), true);
		}
	}

	pub(super) fn on_voice_connected(
		&mut self,
		generation: u64,
		result: Result<Arc<VoiceConnection>, VoiceError>,
	) {
		if generation != self.voice.generation {
			return;
		}
		self.voice.connecting = false;
		match result {
			Ok(connection) => self.adopt_connection(connection),
			Err(error) => {
				tracing::warn!(%error, "voice connection failed");
				self.end_voice(Some(error.to_string()), true);
			}
		}
	}

	pub(super) fn on_voice_event(&mut self, generation: u64, event: VoiceEvent) {
		if generation != self.voice.generation {
			return;
		}
		match event {
			VoiceEvent::Connected { .. } => self.announce(VoiceStatus::Connected),
			VoiceEvent::Speaking { user_id, speaking } => {
				self.apply_local(LocalEvent::Speaking { user_id, speaking });
			}
			VoiceEvent::UserLeft { user_id } => {
				self.apply_local(LocalEvent::Speaking {
					user_id,
					speaking: false,
				});
			}
			VoiceEvent::Disconnected {
				reconnecting: true, ..
			} => self.announce(VoiceStatus::Reconnecting),
			VoiceEvent::Disconnected { code, .. } => {
				self.end_voice(Some(close_reason(code)), true);
			}
			VoiceEvent::VideoStarted { .. } | VoiceEvent::VideoStopped { .. } => self.on_remote_video(&event),
			VoiceEvent::Stats {
				ping_ms,
				packets_lost,
				jitter_ms,
			} => {
				let (e2ee, video_bitrate_bps) =
					self.voice.connection.as_ref().map_or((false, 0), |connection| {
						(connection.dave_enabled(), connection.stats().video_bitrate_bps)
					});
				self.apply_local(LocalEvent::VoiceStats {
					ping_ms: Some(ping_ms),
					loss: packets_lost,
					jitter: jitter_ms,
					e2ee,
					video_bitrate_bps,
				});
			}
			_ => {}
		}
	}

	pub(super) fn on_voice_levels(&mut self, generation: u64, input: f32, output: f32) {
		if generation == self.voice.generation {
			self.apply_local(LocalEvent::VoiceLevels { input, output });
		}
	}

	fn announce(&mut self, status: VoiceStatus) {
		let Some(target) = self.voice.target else {
			return;
		};
		self.apply_local(LocalEvent::VoiceConnection {
			status,
			channel_id: target.channel_id,
			guild_id: target.guild_id,
		});
	}

	fn publish_self_state(&mut self) {
		self.apply_local(LocalEvent::VoiceSelf {
			self_mute: self.voice.self_mute,
			self_deaf: self.voice.self_deaf,
		});
		self.resend_voice_state();
	}

	fn send_voice_state(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
	) -> Result<(), SendError> {
		self.gateway.voice_state_video(
			guild_id,
			channel_id,
			self.voice.self_mute,
			self.voice.self_deaf,
			self.voice.camera,
		)
	}

	fn end_voice(&mut self, reason: Option<String>, notify_gateway: bool) {
		let Some(target) = self.voice.target.take() else {
			return;
		};
		self.end_all_streams();
		self.voice.camera = false;
		self.drop_connection();
		self.voice.session_id = None;
		self.voice.server = None;
		if notify_gateway && let Err(error) = self.send_voice_state(target.guild_id, None) {
			tracing::warn!(%error, "voice state was not sent");
		}
		self.apply_local(LocalEvent::VoiceConnection {
			status: VoiceStatus::Disconnected { reason },
			channel_id: target.channel_id,
			guild_id: target.guild_id,
		});
	}

	fn drop_connection(&mut self) {
		self.voice.generation += 1;
		self.voice.connecting = false;
		if let Some(pump) = self.voice.pump.take() {
			pump.abort();
		}
		if let Some(connection) = self.voice.connection.take() {
			connection.disconnect();
			self.video.lock().detach(&connection);
		}
		*self.voice.shared.lock() = None;
	}

	fn try_connect(&mut self) {
		if self.voice.connecting || self.voice.connection.is_some() {
			return;
		}
		let (Some(target), Some(session_id), Some(server), Some(user_id)) = (
			self.voice.target,
			self.voice.session_id.clone(),
			self.voice.server.as_ref(),
			self.state.read().current_user_id(),
		) else {
			return;
		};
		let params = VoiceParams {
			guild_id: target.guild_id,
			channel_id: target.channel_id,
			user_id,
			session_id,
			token: Token::new(server.token.as_str()),
			endpoint: server.endpoint.clone(),
		};
		let options = self.voice_options();
		self.voice.connecting = true;
		let generation = self.voice.generation;
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let result = VoiceConnection::connect_with(params, options).await;
			let _ = internal.send(Internal::VoiceConnected {
				generation,
				result: result.map(Arc::new),
			});
		});
	}

	fn voice_options(&self) -> VoiceOptions {
		let settings = self.store.settings.read();
		VoiceOptions {
			audio_io: self.voice.io == VoiceIo::Hardware,
			input_device: settings.voice.input_device.clone(),
			output_device: settings.voice.output_device.clone(),
			..VoiceOptions::default()
		}
	}

	fn adopt_connection(&mut self, connection: Arc<VoiceConnection>) {
		{
			let settings = self.store.settings.read();
			let voice = &settings.voice;
			connection.set_input_volume(f32::from(voice.input_volume) / FULL_VOLUME_PERCENT);
			connection.set_output_volume(f32::from(voice.output_volume) / FULL_VOLUME_PERCENT);
			connection.set_noise_suppression(voice.noise_suppression);
			connection.set_vad_threshold_db(vad_threshold_db(
				voice.auto_sensitivity,
				f32::from(voice.sensitivity_db),
			));
			connection.set_input_mode(input_mode(voice.push_to_talk));
		}
		if let Some(quality) = self.voice.default_video_quality {
			connection.set_video_quality(quality);
		}
		connection.set_transmitting(self.voice.transmitting);
		connection.set_mute(self.voice.self_mute);
		connection.set_deaf(self.voice.self_deaf);
		for (user_id, volume) in &self.voice.user_volumes {
			connection.set_user_volume(*user_id, *volume);
		}
		*self.voice.shared.lock() = Some(connection.clone());
		self.video.lock().attach(&connection);
		self.voice.pump = Some(self.spawn_pump(connection.clone()));
		self.voice.connection = Some(connection);
	}

	fn spawn_pump(&self, connection: Arc<VoiceConnection>) -> JoinHandle<()> {
		let generation = self.voice.generation;
		let internal = self.internal.clone();
		let mut events = connection.events();
		self.runtime.spawn(async move {
			let mut ticker = interval(LEVEL_INTERVAL);
			ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
			let mut last = (0.0, 0.0);
			loop {
				let message = tokio::select! {
					event = events.recv() => match event {
						Some(event) => Internal::VoiceEvent { generation, event },
						None => break,
					},
					_ = ticker.tick() => {
						let levels = (
							settled(connection.input_level()),
							settled(connection.output_level()),
						);
						if levels == last {
							continue;
						}
						last = levels;
						Internal::VoiceLevels {
							generation,
							input: levels.0,
							output: levels.1,
						}
					}
				};
				if internal.send(message).is_err() {
					break;
				}
			}
		})
	}

	fn update_voice_settings(&self, apply: impl FnOnce(&mut VoiceSettings) + Send + 'static) {
		let store = self.store.clone();
		self.runtime.spawn_blocking(move || {
			if let Err(error) = store.update_settings(|settings| apply(&mut settings.voice)) {
				tracing::warn!(%error, "voice settings were not saved");
			}
		});
	}
}
