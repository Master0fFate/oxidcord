mod audio;
mod engine;
mod error;
mod output;
mod source;
mod video;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use media::MediaFetcher;
use parking_lot::Mutex;
use tokio::runtime::Handle;

pub use error::{PlayerError, Result};
pub use video::codec_pack_available;

pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Frame {
	pub width: u32,
	pub height: u32,
	pub bgra: Vec<u8>,
	pub pts: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
	Loading,
	Playing,
	Paused,
	Ended,
	Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
	pub state: State,
	pub position: Duration,
	pub duration: Option<Duration>,
	pub video: Option<(u32, u32)>,
	pub audio: bool,
	pub buffering: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
	pub autoplay: bool,
	pub looping: bool,
	pub volume: f32,
	pub max_size: (u32, u32),
}

pub(crate) enum Command {
	Play,
	Pause,
	Seek(Duration),
	Resize((u32, u32)),
}

pub(crate) struct Shared {
	status: Mutex<Status>,
	frame: Mutex<Option<Frame>>,
	closed: Arc<AtomicBool>,
	volume: Arc<AtomicU32>,
	wake: Wake,
}

impl Shared {
	fn update(&self, change: impl FnOnce(&mut Status)) {
		let changed = {
			let mut status = self.status.lock();
			let before = *status;
			change(&mut status);
			*status != before
		};
		if changed {
			(self.wake)();
		}
	}

	fn publish(&self, frame: Frame) {
		*self.frame.lock() = Some(frame);
		(self.wake)();
	}

	fn closed(&self) -> bool {
		self.closed.load(Ordering::Relaxed)
	}
}

pub struct Player {
	shared: Arc<Shared>,
	commands: Sender<Command>,
}

impl Player {
	pub fn open(fetcher: MediaFetcher, runtime: Handle, url: String, options: Options, wake: Wake) -> Self {
		let shared = Arc::new(Shared {
			status: Mutex::new(Status {
				state: State::Loading,
				position: Duration::ZERO,
				duration: None,
				video: None,
				audio: false,
				buffering: true,
			}),
			frame: Mutex::new(None),
			closed: Arc::new(AtomicBool::new(false)),
			volume: Arc::new(AtomicU32::new(options.volume.to_bits())),
			wake,
		});
		let (commands, receiver) = mpsc::channel();
		let engine_shared = shared.clone();
		let spawned = thread::Builder::new()
			.name("media-player".to_owned())
			.spawn(move || engine::run(engine_shared, receiver, fetcher, runtime, url, options));
		if let Err(error) = spawned {
			tracing::warn!(%error, "could not start a media player thread");
			shared.update(|status| status.state = State::Failed);
		}
		Self { shared, commands }
	}

	fn send(&self, command: Command) {
		let _ = self.commands.send(command);
	}

	pub fn play(&self) {
		self.send(Command::Play);
	}

	pub fn pause(&self) {
		self.send(Command::Pause);
	}

	pub fn seek(&self, position: Duration) {
		self.send(Command::Seek(position));
	}

	pub fn resize(&self, max_size: (u32, u32)) {
		self.send(Command::Resize(max_size));
	}

	pub fn set_volume(&self, volume: f32) {
		self.shared
			.volume
			.store(volume.clamp(0., 1.).to_bits(), Ordering::Relaxed);
	}

	pub fn status(&self) -> Status {
		*self.shared.status.lock()
	}

	pub fn take_frame(&self) -> Option<Frame> {
		self.shared.frame.lock().take()
	}
}

impl Drop for Player {
	fn drop(&mut self) {
		self.shared.closed.store(true, Ordering::Relaxed);
	}
}
