use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{debug, warn};

use super::capture::VideoSource;
use super::encode::{EncoderConfig, H264Encoder};
use super::hub::OutgoingFrame;
use super::{VideoError, VideoKind, convert, rtp};
use crate::shared::Shared;

const PAUSED_POLL: Duration = Duration::from_millis(50);
const FAILURE_BACKOFF: Duration = Duration::from_millis(250);
const MAX_CONSECUTIVE_FAILURES: u32 = 40;
const DEFAULT_KEYFRAME_SECONDS: u32 = 2;
const BITRATE_WINDOW: Duration = Duration::from_secs(1);
const FRAME_SKIP_RATIO: f64 = 0.8;

pub(crate) struct SenderHandle {
	stop: Arc<AtomicBool>,
	thread: Option<JoinHandle<()>>,
}

impl SenderHandle {
	pub fn spawn(
		source: Box<dyn VideoSource>,
		kind: VideoKind,
		shared: Arc<Shared>,
	) -> Result<Self, VideoError> {
		let stop = Arc::new(AtomicBool::new(false));
		let flag = stop.clone();
		let thread = thread::Builder::new()
			.name("video-send".to_owned())
			.spawn(move || Pipeline::new(source, kind, shared, flag).run())
			.map_err(|error| VideoError::Device(error.to_string()))?;
		Ok(Self {
			stop,
			thread: Some(thread),
		})
	}

	pub fn stop(mut self) -> Option<JoinHandle<()>> {
		self.stop.store(true, Ordering::Release);
		self.thread.take()
	}
}

struct Pipeline {
	source: Box<dyn VideoSource>,
	kind: VideoKind,
	shared: Arc<Shared>,
	stop: Arc<AtomicBool>,
	encoder: Option<H264Encoder>,
	revision: u32,
	started: Instant,
	base_timestamp: u32,
	last_encoded: Option<Instant>,
	last_keyframe: Instant,
	window_started: Instant,
	window_bytes: u64,
	was_paused: bool,
}

impl Pipeline {
	fn new(
		source: Box<dyn VideoSource>,
		kind: VideoKind,
		shared: Arc<Shared>,
		stop: Arc<AtomicBool>,
	) -> Self {
		let now = Instant::now();
		Self {
			source,
			kind,
			shared,
			stop,
			encoder: None,
			revision: u32::MAX,
			started: now,
			base_timestamp: rand::random(),
			last_encoded: None,
			last_keyframe: now,
			window_started: now,
			window_bytes: 0,
			was_paused: false,
		}
	}

	fn run(mut self) {
		let mut failures = 0u32;
		while !self.stop.load(Ordering::Acquire) {
			if !self.shared.video.wanted.load(Ordering::Relaxed) || self.shared.video.ssrc() == 0 {
				self.was_paused = true;
				thread::sleep(PAUSED_POLL);
				continue;
			}
			match self.step() {
				Ok(()) => failures = 0,
				Err(error) => {
					failures += 1;
					warn!(%error, "video pipeline step failed");
					if failures >= MAX_CONSECUTIVE_FAILURES {
						break;
					}
					thread::sleep(FAILURE_BACKOFF);
				}
			}
		}
		self.shared.video.bitrate.store(0, Ordering::Relaxed);
		debug!("video pipeline stopped");
	}

	fn step(&mut self) -> Result<(), VideoError> {
		let frame = self.source.next_frame()?;
		let quality = self.shared.video.quality();
		let revision = self.shared.video.quality_revision.load(Ordering::Acquire);
		if revision != self.revision {
			self.revision = revision;
			self.source.set_fps(quality.fps);
		}
		let now = Instant::now();
		let interval = Duration::from_secs_f64(1.0 / f64::from(quality.fps.max(1)));
		if self.last_encoded.is_some_and(|last| {
			now.duration_since(last).as_secs_f64() < interval.as_secs_f64() * FRAME_SKIP_RATIO
		}) {
			return Ok(());
		}
		self.last_encoded = Some(now);
		let (width, height) = convert::fit(frame.width, frame.height, quality.width, quality.height);
		let config = EncoderConfig {
			width,
			height,
			fps: quality.fps.max(1),
			bitrate: quality.bitrate,
			keyframe_seconds: DEFAULT_KEYFRAME_SECONDS,
			kind: self.kind,
		};
		let interval_ms = self.shared.video.keyframe_interval_ms.load(Ordering::Relaxed);
		let overdue = interval_ms > 0
			&& now.duration_since(self.last_keyframe) >= Duration::from_millis(u64::from(interval_ms));
		let keyframe =
			self.shared.video.take_keyframe_request() | overdue | std::mem::take(&mut self.was_paused);
		let encoder = self.encoder_for(config)?;
		if keyframe {
			encoder.request_keyframe();
		}
		let encoded = encoder.encode(&frame)?;
		let timestamp = self
			.base_timestamp
			.wrapping_add((self.started.elapsed().as_secs_f64() * f64::from(rtp::CLOCK_HZ)) as u64 as u32);
		for unit in encoded {
			if unit.keyframe {
				self.last_keyframe = now;
			}
			self.window_bytes += unit.data.len() as u64;
			let kept = self.shared.video.push(OutgoingFrame {
				data: unit.data,
				timestamp,
			});
			if !kept {
				self.shared.counters.video_dropped.fetch_add(1, Ordering::Relaxed);
			}
		}
		self.publish_bitrate(now);
		Ok(())
	}

	fn encoder_for(&mut self, config: EncoderConfig) -> Result<&mut H264Encoder, VideoError> {
		let rebuild = match &self.encoder {
			Some(encoder) => {
				let current = encoder.config();
				(current.width, current.height, current.fps) != (config.width, config.height, config.fps)
			}
			None => true,
		};
		if rebuild {
			let encoder = H264Encoder::new(config)?;
			debug!(backend = ?encoder.backend(), width = config.width, height = config.height, "video encoder ready");
			self.encoder = Some(encoder);
		}
		let encoder = self.encoder.as_mut().ok_or(VideoError::NotFound)?;
		encoder.set_bitrate(config.bitrate)?;
		Ok(encoder)
	}

	fn publish_bitrate(&mut self, now: Instant) {
		let elapsed = now.duration_since(self.window_started);
		if elapsed < BITRATE_WINDOW {
			return;
		}
		let bits_per_second = (self.window_bytes as f64 * 8.0 / elapsed.as_secs_f64()) as u32;
		self.shared
			.video
			.bitrate
			.store(bits_per_second, Ordering::Relaxed);
		self.window_started = now;
		self.window_bytes = 0;
	}
}
