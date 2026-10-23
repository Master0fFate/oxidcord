use std::sync::Arc;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use protocol::{Snowflake, Token};
use tokio::sync::{mpsc, watch};

use crate::audio::{AudioEngine, Rings};
use crate::dave::DaveSession;
use crate::error::{Result, VoiceError};
use crate::event::VoiceEvent;
use crate::media::SharedTransform;
use crate::session::{Credentials, Signals, Supervisor, establish};
use crate::shared::{Control, InputMode, Shared, VideoControl, VoiceStats};
use crate::transform::{FrameTransform, Layered, Passthrough};
use crate::video::capture::{CameraSource, ScreenSource, ScreenTarget, VideoSource};
use crate::video::send::SenderHandle;
use crate::video::{VideoError, VideoKind, VideoQuality, VideoSink};

const MAX_VOLUME: f32 = 2.0;

#[derive(Debug)]
pub struct VoiceParams {
	pub guild_id: Option<Snowflake>,
	pub channel_id: Snowflake,
	pub user_id: Snowflake,
	pub session_id: String,
	pub token: Token,
	pub endpoint: String,
}

pub struct VoiceOptions {
	pub audio_io: bool,
	pub input_device: Option<String>,
	pub output_device: Option<String>,
	pub transform: Box<dyn FrameTransform>,
	pub stream: bool,
}

impl Default for VoiceOptions {
	fn default() -> Self {
		Self {
			audio_io: true,
			input_device: None,
			output_device: None,
			transform: Box::new(Passthrough),
			stream: false,
		}
	}
}

pub struct VoiceConnection {
	shared: Arc<Shared>,
	shutdown: watch::Sender<bool>,
	control: mpsc::UnboundedSender<Control>,
}

impl VoiceConnection {
	pub async fn connect(params: VoiceParams) -> Result<Self> {
		Self::connect_with(params, VoiceOptions::default()).await
	}

	pub async fn connect_with(params: VoiceParams, options: VoiceOptions) -> Result<Self> {
		let credentials = Credentials::new(
			params.guild_id.unwrap_or(params.channel_id),
			params.user_id,
			params.session_id,
			params.token,
			&params.endpoint,
			options.stream,
		)?;
		let established = establish(&credentials).await?;
		let dave_group = match (options.stream, params.guild_id) {
			(true, Some(server_id)) => server_id.get().saturating_sub(1),
			_ => params.channel_id.get(),
		};
		let dave = DaveSession::new(params.user_id.get(), dave_group)?;
		let rings = Arc::new(Rings::new());
		let shared = Arc::new(Shared::new(rings.clone(), dave.stats(), options.stream));
		if options.audio_io {
			let engine = tokio::task::spawn_blocking(move || {
				AudioEngine::start(rings, options.input_device, options.output_device)
			})
			.await
			.map_err(|error| VoiceError::Audio(error.to_string()))??;
			*shared.engine.lock() = Some(engine);
		}
		let resolver = shared.clone();
		let dave_transform =
			dave.transform(move |ssrc| resolver.ssrc_users.lock().get(&ssrc).map(|user| user.get()));
		let transform: SharedTransform = Arc::new(Mutex::new(Box::new(Layered::new(
			dave_transform,
			options.transform,
		))));
		let (shutdown, shutdown_receiver) = watch::channel(false);
		let (control, commands) = mpsc::unbounded_channel();
		let supervisor = Supervisor::start(
			credentials,
			established,
			shared.clone(),
			transform,
			dave,
			options.audio_io,
			Signals {
				shutdown: shutdown_receiver,
				control: control.clone(),
				commands,
			},
		)?;
		tokio::spawn(supervisor.run());
		Ok(Self {
			shared,
			shutdown,
			control,
		})
	}

	pub fn events(&self) -> mpsc::Receiver<VoiceEvent> {
		self.shared.hub.subscribe()
	}

	pub fn ssrc(&self) -> Option<u32> {
		self.shared.hub.ssrc()
	}

	pub fn set_mute(&self, muted: bool) {
		self.shared.muted.store(muted, Ordering::Relaxed);
	}

	pub fn set_deaf(&self, deafened: bool) {
		self.shared.deafened.store(deafened, Ordering::Relaxed);
	}

	pub fn set_input_device(&self, id: Option<String>) {
		if let Some(engine) = self.shared.engine.lock().as_ref() {
			engine.set_input(id);
		}
	}

	pub fn set_output_device(&self, id: Option<String>) {
		if let Some(engine) = self.shared.engine.lock().as_ref() {
			engine.set_output(id);
		}
	}

	pub fn set_user_volume(&self, user_id: Snowflake, volume: f32) {
		self.shared
			.user_volumes
			.lock()
			.insert(user_id, volume.clamp(0.0, MAX_VOLUME));
	}

	pub fn set_input_volume(&self, volume: f32) {
		self.shared.input_volume.store(volume.clamp(0.0, MAX_VOLUME));
	}

	pub fn set_output_volume(&self, volume: f32) {
		self.shared.output_volume.store(volume.clamp(0.0, MAX_VOLUME));
	}

	pub fn set_input_mode(&self, mode: InputMode) {
		self.shared.set_input_mode(mode);
	}

	pub fn set_vad_threshold_db(&self, threshold_db: f32) {
		self.shared.vad_threshold_db.store(threshold_db);
	}

	pub fn set_transmitting(&self, transmitting: bool) {
		self.shared.transmitting.store(transmitting, Ordering::Relaxed);
	}

	pub fn set_noise_suppression(&self, enabled: bool) {
		self.shared.noise_suppression.store(enabled, Ordering::Relaxed);
	}

	pub fn input_level(&self) -> f32 {
		self.shared.input_level.load()
	}

	pub fn output_level(&self) -> f32 {
		self.shared.output_level.load()
	}

	pub fn stats(&self) -> VoiceStats {
		self.shared
			.counters
			.snapshot(self.shared.video.bitrate.load(Ordering::Relaxed))
	}

	pub fn set_video_sink(&self, sink: Arc<dyn VideoSink>) {
		*self.shared.video.sink.lock() = Some(sink);
	}

	pub fn clear_video_sink(&self) {
		*self.shared.video.sink.lock() = None;
	}

	pub async fn start_camera(&self, source_id: &str, quality: VideoQuality) -> Result<()> {
		let id = source_id.to_owned();
		self.start_video(VideoKind::Camera, quality, move || {
			let source = CameraSource::open(&id, quality.width, quality.height, quality.fps)?;
			Ok(Box::new(source))
		})
		.await
	}

	pub fn stop_camera(&self) {
		self.stop_video(VideoKind::Camera);
	}

	pub async fn start_screen_share(
		&self,
		target: &ScreenTarget,
		quality: VideoQuality,
		with_audio: bool,
	) -> Result<()> {
		let target = target.clone();
		self.start_video(VideoKind::Screen, quality, move || {
			let source = ScreenSource::open(&target, quality.fps)?;
			Ok(Box::new(source))
		})
		.await?;
		if with_audio && let Err(error) = self.set_share_audio(true) {
			tracing::warn!(%error, "system audio capture failed, sharing video only");
		}
		Ok(())
	}

	pub fn stop_screen_share(&self) {
		self.stop_video(VideoKind::Screen);
	}

	pub fn set_share_audio(&self, enabled: bool) -> Result<()> {
		#[cfg(windows)]
		{
			match self.shared.engine.lock().as_ref() {
				Some(engine) => engine.set_system_audio(enabled),
				None if enabled => Err(VoiceError::Audio("audio is not running".to_owned())),
				None => Ok(()),
			}
		}
		#[cfg(not(windows))]
		{
			if enabled {
				Err(VideoError::Unsupported.into())
			} else {
				Ok(())
			}
		}
	}

	pub fn set_video_quality(&self, quality: VideoQuality) {
		let video = &self.shared.video;
		if video.sender.lock().is_none() {
			return;
		}
		video.set_quality(quality);
		let kind = video.active_kind();
		let _ = self
			.control
			.send(Control::Video(VideoControl::Announce { kind, quality }));
	}

	pub fn active_video(&self) -> Option<VideoKind> {
		self.shared
			.video
			.sender
			.lock()
			.is_some()
			.then(|| self.shared.video.active_kind())
	}

	async fn start_video(
		&self,
		kind: VideoKind,
		quality: VideoQuality,
		open: impl FnOnce() -> std::result::Result<Box<dyn VideoSource>, VideoError> + Send + 'static,
	) -> Result<()> {
		let video = &self.shared.video;
		if video.starting.swap(true, Ordering::AcqRel) || video.sender.lock().is_some() {
			video.starting.store(false, Ordering::Release);
			return Err(VideoError::Busy.into());
		}
		let retired = video.retired.lock().take();
		let opened = tokio::task::spawn_blocking(move || {
			if let Some(thread) = retired {
				let _ = thread.join();
			}
			open()
		})
		.await;
		let result = match opened {
			Ok(Ok(source)) => self.launch_video(source, kind, quality),
			Ok(Err(error)) => Err(error.into()),
			Err(error) => Err(VoiceError::Audio(error.to_string())),
		};
		video.starting.store(false, Ordering::Release);
		result
	}

	fn launch_video(
		&self,
		source: Box<dyn VideoSource>,
		kind: VideoKind,
		quality: VideoQuality,
	) -> Result<()> {
		let video = &self.shared.video;
		video.set_quality(quality);
		video.set_active_kind(kind);
		let sender = SenderHandle::spawn(source, kind, self.shared.clone())?;
		*video.sender.lock() = Some(sender);
		self.control
			.send(Control::Video(VideoControl::Announce { kind, quality }))
			.map_err(|_| VoiceError::Transport("voice connection closed".to_owned()))
	}

	fn stop_video(&self, kind: VideoKind) {
		let video = &self.shared.video;
		if video.active_kind() == kind && video.stop_sender() {
			if kind == VideoKind::Screen {
				let _ = self.set_share_audio(false);
			}
			let _ = self.control.send(Control::Video(VideoControl::Clear));
		}
	}

	pub fn dave_enabled(&self) -> bool {
		self.shared.counters.dave.enabled()
	}

	pub fn disconnect(&self) {
		self.shared.video.stop_sender();
		let _ = self.shutdown.send(true);
	}

	#[cfg(feature = "testing")]
	pub fn push_test_frame(&self, pcm: &[i16]) {
		let frame = pcm
			.iter()
			.map(|sample| f32::from(*sample) / f32::from(i16::MAX))
			.collect();
		self.shared.test_input.lock().push_back(frame);
	}

	#[cfg(feature = "testing")]
	pub fn take_output_frames(&self) -> Vec<Vec<i16>> {
		std::mem::take(&mut *self.shared.test_output.lock())
	}
}

impl Drop for VoiceConnection {
	fn drop(&mut self) {
		self.shared.video.stop_sender();
		let _ = self.shutdown.send(true);
	}
}
