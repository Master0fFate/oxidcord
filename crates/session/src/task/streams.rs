use std::sync::Arc;
use std::time::Duration;

use protocol::{Snowflake, StreamDelete, StreamServerUpdate, StreamState, Token};
use state::{LocalEvent, StreamInfo};
use tokio::time::sleep;
use voice::video::VideoQuality;
use voice::video::capture::ScreenTarget;
use voice::{VoiceConnection, VoiceError, VoiceEvent, VoiceOptions, VoiceParams};

use super::{Core, Internal};

const STREAM_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
enum Role {
	Share {
		target: ScreenTarget,
		quality: VideoQuality,
		with_audio: bool,
	},
	Watch,
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
}

#[derive(Default)]
pub(crate) struct StreamLink {
	generation: u64,
	share: Option<StreamSession>,
	watch: Option<StreamSession>,
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
			Role::Watch => self.apply_local(LocalEvent::Watching(key)),
		}
	}

	pub(super) fn on_stream_video(&mut self, generation: u64, result: Result<(), String>) {
		if let Err(error) = result {
			self.fail_stream(generation, &format!("Screen share failed: {error}"));
		}
	}

	pub(super) fn on_stream_event(&mut self, generation: u64, event: VoiceEvent) {
		match event {
			VoiceEvent::Disconnected {
				reconnecting: false, ..
			} => self.fail_stream(generation, "Stream connection lost"),
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
			audio_io: false,
			stream: true,
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
		if let Some(pump) = session.pump {
			pump.abort();
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
