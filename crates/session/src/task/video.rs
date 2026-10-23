use std::sync::Arc;

use parking_lot::Mutex;
use state::{LocalEvent, VideoKind};
use voice::video::VideoQuality;
use voice::video::VideoSink;
use voice::video::capture::CameraSource;
use voice::{VoiceConnection, VoiceEvent};

use super::{Core, Internal};

#[derive(Default)]
pub(crate) struct VideoRouting {
	sink: Option<Arc<dyn VideoSink>>,
	connections: Vec<Arc<VoiceConnection>>,
}

impl VideoRouting {
	pub(crate) fn set_sink(&mut self, sink: Arc<dyn VideoSink>) {
		for connection in &self.connections {
			connection.set_video_sink(sink.clone());
		}
		self.sink = Some(sink);
	}

	pub(crate) fn attach(&mut self, connection: &Arc<VoiceConnection>) {
		if let Some(sink) = &self.sink {
			connection.set_video_sink(sink.clone());
		}
		self.connections.push(connection.clone());
	}

	pub(crate) fn detach(&mut self, connection: &Arc<VoiceConnection>) {
		self.connections
			.retain(|attached| !Arc::ptr_eq(attached, connection));
	}
}

pub(crate) type SharedVideo = Arc<Mutex<VideoRouting>>;

pub(super) fn state_kind(kind: voice::video::VideoKind) -> VideoKind {
	match kind {
		voice::video::VideoKind::Camera => VideoKind::Camera,
		voice::video::VideoKind::Screen => VideoKind::Screen,
	}
}

async fn default_camera() -> Result<String, String> {
	let cameras = tokio::task::spawn_blocking(CameraSource::list)
		.await
		.map_err(|error| error.to_string())?
		.map_err(|error| error.to_string())?;
	cameras
		.into_iter()
		.next()
		.map(|camera| camera.id)
		.ok_or_else(|| "No camera found".to_owned())
}

impl Core {
	pub(super) fn start_camera(&mut self, device_id: Option<String>, quality: VideoQuality) {
		let Some(connection) = self.voice.connection.clone() else {
			self.reject("Join a voice channel first");
			return;
		};
		let generation = self.voice.generation;
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let id = match device_id {
				Some(id) => Ok(id),
				None => default_camera().await,
			};
			let result = match id {
				Ok(id) => connection
					.start_camera(&id, quality)
					.await
					.map_err(|error| error.to_string()),
				Err(error) => Err(error),
			};
			let _ = internal.send(Internal::CameraStarted { generation, result });
		});
	}

	pub(super) fn stop_camera(&mut self) {
		if let Some(connection) = &self.voice.connection {
			connection.stop_camera();
		}
		self.set_camera(false);
	}

	pub(super) fn on_camera_started(&mut self, generation: u64, result: Result<(), String>) {
		if generation != self.voice.generation {
			return;
		}
		match result {
			Ok(()) => self.set_camera(true),
			Err(error) => self.reject(format!("Camera failed: {error}")),
		}
	}

	pub(super) fn set_video_quality(&self, quality: VideoQuality) {
		if let Some(connection) = &self.voice.connection {
			connection.set_video_quality(quality);
		}
		self.streams.set_quality(quality);
	}

	pub(super) fn set_camera(&mut self, on: bool) {
		if self.voice.camera == on {
			return;
		}
		self.voice.camera = on;
		self.apply_local(LocalEvent::CameraChanged(on));
		self.resend_voice_state();
	}

	pub(super) fn on_remote_video(&mut self, event: &VoiceEvent) {
		let (user_id, kind, active) = match event {
			VoiceEvent::VideoStarted { user_id, kind, .. } => (*user_id, *kind, true),
			VoiceEvent::VideoStopped { user_id, kind, .. } => (*user_id, *kind, false),
			_ => return,
		};
		self.apply_local(LocalEvent::RemoteVideo {
			user_id,
			kind: state_kind(kind),
			active,
		});
	}
}
