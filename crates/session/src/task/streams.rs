use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder as _};
use protocol::{Snowflake, StreamDelete, StreamServerUpdate, StreamState, Token};
use rest::RestClient;
use rest::api::image_data_uri;
use state::{LocalEvent, StreamInfo};
use tokio::time::sleep;
use voice::video::capture::ScreenTarget;
use voice::video::{VideoFrame, VideoQuality, scaled_bgra};
use voice::{AudioRoute, ShareAudio, VoiceConnection, VoiceError, VoiceEvent, VoiceOptions, VoiceParams};

use super::voice_link::gain;
use super::{Core, Internal};
use crate::config::VoiceIo;

const STREAM_TIMEOUT: Duration = Duration::from_secs(15);
const FULL_VOLUME: f32 = 1.0;
const PREVIEW_INTERVAL: Duration = Duration::from_secs(300);
const PREVIEW_RETRY: Duration = Duration::from_secs(30);
const PREVIEW_SIDE: u32 = 512;
const PREVIEW_JPEG_QUALITY: u8 = 70;
const PREVIEW_MIME: &str = "image/jpeg";

#[derive(Debug, Clone)]
enum Role {
	Share {
		target: ScreenTarget,
		quality: VideoQuality,
		with_audio: bool,
	},
	Watch,
}

fn audio_route(io: VoiceIo, role: &Role) -> AudioRoute {
	match (io, role) {
		(VoiceIo::Silent, _)
		| (
			_,
			Role::Share {
				with_audio: false, ..
			},
		) => AudioRoute::Silent,
		(VoiceIo::Hardware, Role::Share { .. }) => AudioRoute::Share,
		(VoiceIo::Hardware, Role::Watch) => AudioRoute::Watch,
	}
}

struct Server {
	token: Token,
	endpoint: String,
}

struct StreamSession {
	role: Role,
	generation: u64,
	key: Option<String>,
	rtc_server_id: Option<Snowflake>,
	rtc_channel_id: Snowflake,
	server: Option<Server>,
	connection: Option<Arc<VoiceConnection>>,
	connecting: bool,
	pump: Option<tokio::task::JoinHandle<()>>,
	uploads: Option<tokio::task::JoinHandle<()>>,
}

pub(crate) struct StreamLink {
	generation: u64,
	volume: f32,
	share: Option<StreamSession>,
	watch: Option<StreamSession>,
	previews: HashSet<String>,
}

impl Default for StreamLink {
	fn default() -> Self {
		Self {
			generation: 0,
			volume: FULL_VOLUME,
			share: None,
			watch: None,
			previews: HashSet::new(),
		}
	}
}

fn encode_preview(frame: &VideoFrame) -> Option<String> {
	let scaled = scaled_bgra(frame, PREVIEW_SIDE)?;
	let rgb: Vec<u8> = scaled
		.data
		.as_chunks::<4>()
		.0
		.iter()
		.flat_map(|&[blue, green, red, _]| [red, green, blue])
		.collect();
	let mut jpeg = Vec::new();
	JpegEncoder::new_with_quality(&mut jpeg, PREVIEW_JPEG_QUALITY)
		.write_image(&rgb, scaled.width, scaled.height, ExtendedColorType::Rgb8)
		.ok()?;
	Some(image_data_uri(PREVIEW_MIME, &jpeg))
}

async fn upload_preview(rest: &RestClient, connection: &VoiceConnection, stream_key: &str) {
	let Some(frame) = connection.capture_preview().await else {
		return;
	};
	let Some(thumbnail) = tokio::task::spawn_blocking(move || encode_preview(&frame))
		.await
		.ok()
		.flatten()
	else {
		return;
	};
	if let Err(error) = rest.upload_stream_preview(stream_key, &thumbnail).await {
		tracing::warn!(%error, "stream preview was not uploaded");
	}
}

impl StreamLink {
	pub(super) fn set_quality(&self, quality: VideoQuality) {
		if let Some(connection) = self
			.share
			.as_ref()
			.and_then(|session| session.connection.as_ref())
		{
			connection.set_video_quality(quality);
		}
	}

	pub(super) fn set_deaf(&self, deafened: bool) {
		if let Some(connection) = self.watched_connection() {
			connection.set_deaf(deafened);
		}
	}

	pub(super) fn set_output_device(&self, output: Option<String>) {
		if let Some(connection) = self.watched_connection() {
			connection.set_output_device(output);
		}
	}

	fn watched_connection(&self) -> Option<&Arc<VoiceConnection>> {
		self.watch
			.as_ref()
			.and_then(|session| session.connection.as_ref())
	}

	fn next_generation(&mut self) -> u64 {
		self.generation += 1;
		self.generation
	}

	fn session_mut(&mut self, generation: u64) -> Option<&mut StreamSession> {
		[self.share.as_mut(), self.watch.as_mut()]
			.into_iter()
			.flatten()
			.find(|session| session.generation == generation)
	}

	fn session_for_key(&mut self, key: &str) -> Option<&mut StreamSession> {
		[self.share.as_mut(), self.watch.as_mut()]
			.into_iter()
			.flatten()
			.find(|session| session.key.as_deref() == Some(key))
	}
}

impl Core {
	pub(super) fn start_screen_share(
		&mut self,
		target: ScreenTarget,
		quality: VideoQuality,
		with_audio: bool,
	) {
		let Some(voice_target) = self.voice.target() else {
			self.reject("Join a voice channel first");
			return;
		};
		if self.streams.share.is_some() {
			self.reject("Already sharing your screen");
			return;
		}
		if let Err(error) = self
			.gateway
			.stream_create(voice_target.guild_id, voice_target.channel_id, None)
		{
			tracing::warn!(%error, "stream create was not sent");
			self.reject("Could not start the stream");
			return;
		}
		let generation = self.streams.next_generation();
		self.streams.share = Some(StreamSession {
			role: Role::Share {
				target,
				quality,
				with_audio,
			},
			generation,
			key: None,
			rtc_server_id: None,
			rtc_channel_id: voice_target.channel_id,
			server: None,
			connection: None,
			connecting: false,
			pump: None,
			uploads: None,
		});
		self.arm_stream_timeout(generation);
	}

	pub(super) fn stop_screen_share(&mut self) {
		if let Some(session) = self.streams.share.take() {
			self.end_stream_session(session, true);
		}
	}

	pub(super) fn watch_stream(&mut self, stream_key: String) {
		if let Some(previous) = self.streams.watch.take() {
			self.end_stream_session(previous, true);
		}
		if let Err(error) = self.gateway.stream_watch(&stream_key) {
			tracing::warn!(%error, "stream watch was not sent");
			self.reject("Could not watch the stream");
			return;
		}
		let generation = self.streams.next_generation();
		self.streams.watch = Some(StreamSession {
			role: Role::Watch,
			generation,
			key: Some(stream_key),
			rtc_server_id: None,
			rtc_channel_id: Snowflake::default(),
			server: None,
			connection: None,
			connecting: false,
			pump: None,
			uploads: None,
		});
		self.arm_stream_timeout(generation);
	}

	pub(super) fn stop_watching_stream(&mut self, stream_key: &str) {
		if self
			.streams
			.watch
			.as_ref()
			.is_some_and(|session| session.key.as_deref() == Some(stream_key))
			&& let Some(session) = self.streams.watch.take()
		{
			self.end_stream_session(session, true);
		}
	}

	pub(super) fn end_all_streams(&mut self) {
		for session in [self.streams.share.take(), self.streams.watch.take()]
			.into_iter()
			.flatten()
		{
			self.end_stream_session(session, true);
		}
	}

	pub(super) fn stream_created(&mut self, stream: &StreamState) {
		let me = self.state.read().current_user_id();
		let owned = me.is_some_and(|me| stream.stream_key.ends_with(&format!(":{me}")));
		let share_pending = owned
			&& self
				.streams
				.share
				.as_ref()
				.is_some_and(|session| session.key.is_none());
		let session = if share_pending {
			self.streams.share.as_mut()
		} else {
			self.streams.session_for_key(&stream.stream_key)
		};
		let Some(session) = session else {
			return;
		};
		session.key = Some(stream.stream_key.clone());
		session.rtc_server_id = stream.rtc_server_id.or(session.rtc_server_id);
		if let Some(channel_id) = stream.rtc_channel_id {
			session.rtc_channel_id = channel_id;
		}
		let generation = session.generation;
		self.try_connect_stream(generation);
	}

	pub(super) fn stream_server_updated(&mut self, update: StreamServerUpdate) {
		let Some(session) = self.streams.session_for_key(&update.stream_key) else {
			return;
		};
		let generation = session.generation;
		match (update.token, update.endpoint) {
			(Some(token), Some(endpoint)) => {
				if let Some(connection) = session.connection.take() {
					connection.disconnect();
					self.video.lock().detach(&connection);
				}
				if let Some(session) = self.streams.session_mut(generation) {
					session.connecting = false;
					session.server = Some(Server { token, endpoint });
				}
				self.try_connect_stream(generation);
			}
			_ => {
				if let Some(connection) = session.connection.take() {
					connection.disconnect();
					self.video.lock().detach(&connection);
				}
				session.server = None;
				session.connecting = false;
			}
		}
	}

	pub(super) fn stream_deleted(&mut self, delete: &StreamDelete) {
		let session = if self
			.streams
			.share
			.as_ref()
			.is_some_and(|session| session.key.as_deref() == Some(delete.stream_key.as_str()))
		{
			self.streams.share.take()
		} else if self
			.streams
			.watch
			.as_ref()
			.is_some_and(|session| session.key.as_deref() == Some(delete.stream_key.as_str()))
		{
			self.streams.watch.take()
		} else {
			None
		};
		if let Some(session) = session {
			self.end_stream_session(session, false);
		}
	}

	pub(super) fn on_stream_timeout(&mut self, generation: u64) {
		let Some(session) = self.streams.session_mut(generation) else {
			return;
		};
		if session.connection.is_some() || session.connecting {
			return;
		}
		let session = if self
			.streams
			.share
			.as_ref()
			.is_some_and(|share| share.generation == generation)
		{
			self.streams.share.take()
		} else {
			self.streams.watch.take()
		};
		if let Some(session) = session {
			self.end_stream_session(session, true);
			self.reject("The stream server did not respond");
		}
	}

	pub(super) fn on_stream_connected(
		&mut self,
		generation: u64,
		result: Result<Arc<VoiceConnection>, VoiceError>,
	) {
		let Some(session) = self.streams.session_mut(generation) else {
			if let Ok(connection) = result {
				connection.disconnect();
			}
			return;
		};
		session.connecting = false;
		let connection = match result {
			Ok(connection) => connection,
			Err(error) => {
				tracing::warn!(%error, "stream connection failed");
				self.fail_stream(generation, &error.to_string());
				return;
			}
		};
		session.connection = Some(connection.clone());
		let role = session.role.clone();
		let key = session.key.clone();
		self.video.lock().attach(&connection);
		let pump = self.spawn_stream_pump(generation, &connection);
		if let Some(session) = self.streams.session_mut(generation) {
			session.pump = Some(pump);
		}
		match role {
			Role::Share {
				target,
				quality,
				with_audio,
			} => {
				let internal = self.internal.clone();
				self.runtime.spawn(async move {
					let result = connection
						.start_screen_share(&target, quality, with_audio)
						.await
						.map_err(|error| error.to_string());
					let _ = internal.send(Internal::StreamVideo { generation, result });
				});
			}
			Role::Watch => {
				self.apply_local(LocalEvent::Watching(key));
			}
		}
	}

	pub(super) fn on_stream_video(&mut self, generation: u64, result: Result<ShareAudio, String>) {
		let audio = match result {
			Ok(audio) => audio,
			Err(error) => {
				self.fail_stream(generation, &format!("Screen share failed: {error}"));
				return;
			}
		};
		let Some(session) = self.streams.session_mut(generation) else {
			return;
		};
		if let (Some(key), Some(connection)) = (session.key.clone(), session.connection.clone()) {
			let rest = self.rest.clone();
			session.uploads = Some(self.runtime.spawn(async move {
				loop {
					upload_preview(&rest, &connection, &key).await;
					sleep(PREVIEW_INTERVAL).await;
				}
			}));
		}
		if audio == ShareAudio::Unavailable {
			self.reject("Sharing without audio");
		}
	}

	pub(super) fn set_stream_volume(&mut self, volume: f32) {
		self.streams.volume = volume;
		self.apply_stream_gain(self.master_volume());
	}

	pub(super) fn apply_stream_gain(&self, master: f32) {
		if let Some(connection) = self.streams.watched_connection() {
			connection.set_output_volume(self.streams.volume * master);
		}
	}

	fn master_volume(&self) -> f32 {
		gain(self.store.settings.read().voice.output_volume)
	}

	pub(super) fn load_stream_preview(&mut self, stream_key: String) {
		if !self.streams.previews.insert(stream_key.clone()) {
			return;
		}
		let rest = self.rest.clone();
		let state = self.state.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			loop {
				let delay = match rest.stream_preview(&stream_key).await {
					Ok(Some(url)) => {
						let _ = internal.send(Internal::StreamPreview {
							stream_key: stream_key.clone(),
							url,
						});
						PREVIEW_INTERVAL
					}
					Ok(None) => PREVIEW_RETRY,
					Err(error) => {
						tracing::warn!(%error, "stream preview was not loaded");
						PREVIEW_RETRY
					}
				};
				sleep(delay).await;
				if !state
					.read()
					.streams()
					.any(|stream| stream.stream_key == stream_key)
				{
					break;
				}
			}
			let _ = internal.send(Internal::StreamPreviewEnded { stream_key });
		});
	}

	pub(super) fn on_stream_preview_ended(&mut self, stream_key: &str) {
		self.streams.previews.remove(stream_key);
	}

	pub(super) fn on_stream_event(&mut self, generation: u64, event: VoiceEvent) {
		match event {
			VoiceEvent::Disconnected {
				reconnecting: false, ..
			} => self.fail_stream(generation, "Stream connection lost"),
			VoiceEvent::VideoFailed { .. } => self.fail_stream(generation, "Screen share ended"),
			event => self.on_remote_video(&event),
		}
	}

	fn fail_stream(&mut self, generation: u64, reason: &str) {
		let session = if self
			.streams
			.share
			.as_ref()
			.is_some_and(|share| share.generation == generation)
		{
			self.streams.share.take()
		} else if self
			.streams
			.watch
			.as_ref()
			.is_some_and(|watch| watch.generation == generation)
		{
			self.streams.watch.take()
		} else {
			None
		};
		if let Some(session) = session {
			self.end_stream_session(session, true);
			self.reject(reason);
		}
	}

	fn arm_stream_timeout(&self, generation: u64) {
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			sleep(STREAM_TIMEOUT).await;
			let _ = internal.send(Internal::StreamTimeout { generation });
		});
	}

	fn try_connect_stream(&mut self, generation: u64) {
		let Some(user_id) = self.state.read().current_user_id() else {
			return;
		};
		let session_id = self.state.read().session().session_id.clone();
		let Some(session) = self.streams.session_mut(generation) else {
			return;
		};
		if session.connecting || session.connection.is_some() {
			return;
		}
		let (Some(rtc_server_id), Some(server)) = (session.rtc_server_id, session.server.as_ref()) else {
			return;
		};
		let params = VoiceParams {
			guild_id: Some(rtc_server_id),
			channel_id: session.rtc_channel_id,
			user_id,
			session_id,
			token: Token::new(server.token.as_str()),
			endpoint: server.endpoint.clone(),
		};
		session.connecting = true;
		let options = VoiceOptions {
			audio: audio_route(self.voice.io(), &session.role),
			output_device: self.store.settings.read().voice.output_device.clone(),
			stream: true,
			deafened: self.voice.self_deaf,
			output_volume: self.streams.volume * self.master_volume(),
			identity: Some(self.identity.clone()),
			..VoiceOptions::default()
		};
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let result = VoiceConnection::connect_with(params, options).await;
			let _ = internal.send(Internal::StreamConnected {
				generation,
				result: result.map(Arc::new),
			});
		});
	}

	fn spawn_stream_pump(
		&self,
		generation: u64,
		connection: &Arc<VoiceConnection>,
	) -> tokio::task::JoinHandle<()> {
		let internal = self.internal.clone();
		let mut events = connection.events();
		self.runtime.spawn(async move {
			while let Some(event) = events.recv().await {
				if internal
					.send(Internal::StreamEvent { generation, event })
					.is_err()
				{
					break;
				}
			}
		})
	}

	fn end_stream_session(&mut self, session: StreamSession, notify_gateway: bool) {
		for task in [session.pump, session.uploads].into_iter().flatten() {
			task.abort();
		}
		if let Some(connection) = session.connection {
			if matches!(session.role, Role::Share { .. }) {
				connection.stop_screen_share();
			}
			connection.disconnect();
			self.video.lock().detach(&connection);
		}
		if notify_gateway
			&& let Some(key) = &session.key
			&& let Err(error) = self.gateway.stream_delete(key)
		{
			tracing::warn!(%error, "stream delete was not sent");
		}
		match session.role {
			Role::Share { .. } => self.apply_local(LocalEvent::ScreenShareChanged(None::<StreamInfo>)),
			Role::Watch => self.apply_local(LocalEvent::Watching(None)),
		}
	}
}
