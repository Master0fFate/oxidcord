use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;

use parking_lot::Mutex;
use tokio::sync::Notify;

use super::send::SenderHandle;
use super::{VideoKind, VideoQuality, VideoSink, VideoStream};

const MAX_QUEUED_FRAMES: usize = 8;

pub(crate) struct OutgoingFrame {
	pub data: Vec<u8>,
	pub timestamp: u32,
}

pub(crate) struct VideoHub {
	pub stream_connection: bool,
	pub sink: Mutex<Option<Arc<dyn VideoSink>>>,
	pub remote: Mutex<HashMap<u32, VideoStream>>,
	pub quality: Mutex<VideoQuality>,
	pub quality_revision: AtomicU32,
	pub sender: Mutex<Option<SenderHandle>>,
	pub retired: Mutex<Option<JoinHandle<()>>>,
	pub starting: AtomicBool,
	active_screen: AtomicBool,
	pub keyframe_wanted: AtomicBool,
	pub wanted: AtomicBool,
	pub keyframe_interval_ms: AtomicU32,
	pub bitrate: AtomicU32,
	pub decode_failures: Mutex<Vec<u32>>,
	pub outgoing_ready: Notify,
	outgoing: Mutex<VecDeque<OutgoingFrame>>,
	ssrc: AtomicU32,
}

impl VideoHub {
	pub fn new(stream_connection: bool) -> Self {
		Self {
			stream_connection,
			sink: Mutex::new(None),
			remote: Mutex::new(HashMap::new()),
			quality: Mutex::new(VideoQuality::default()),
			quality_revision: AtomicU32::new(0),
			sender: Mutex::new(None),
			retired: Mutex::new(None),
			starting: AtomicBool::new(false),
			active_screen: AtomicBool::new(false),
			keyframe_wanted: AtomicBool::new(false),
			wanted: AtomicBool::new(true),
			keyframe_interval_ms: AtomicU32::new(0),
			bitrate: AtomicU32::new(0),
			decode_failures: Mutex::new(Vec::new()),
			outgoing_ready: Notify::new(),
			outgoing: Mutex::new(VecDeque::new()),
			ssrc: AtomicU32::new(0),
		}
	}

	pub fn kind(&self) -> VideoKind {
		if self.stream_connection {
			VideoKind::Screen
		} else {
			VideoKind::Camera
		}
	}

	pub fn active_kind(&self) -> VideoKind {
		if self.active_screen.load(Ordering::Relaxed) {
			VideoKind::Screen
		} else {
			VideoKind::Camera
		}
	}

	pub fn set_active_kind(&self, kind: VideoKind) {
		self.active_screen
			.store(kind == VideoKind::Screen, Ordering::Relaxed);
	}

	pub fn stop_sender(&self) -> bool {
		let Some(sender) = self.sender.lock().take() else {
			return false;
		};
		self.set_ssrc(0);
		*self.retired.lock() = sender.stop();
		true
	}

	pub fn ssrc(&self) -> u32 {
		self.ssrc.load(Ordering::Acquire)
	}

	pub fn set_ssrc(&self, ssrc: u32) {
		self.ssrc.store(ssrc, Ordering::Release);
		if ssrc == 0 {
			self.outgoing.lock().clear();
		} else {
			self.keyframe_wanted.store(true, Ordering::Relaxed);
		}
	}

	pub fn quality(&self) -> VideoQuality {
		*self.quality.lock()
	}

	pub fn set_quality(&self, quality: VideoQuality) {
		*self.quality.lock() = quality;
		self.quality_revision.fetch_add(1, Ordering::Release);
	}

	pub fn push(&self, frame: OutgoingFrame) -> bool {
		let mut queue = self.outgoing.lock();
		let overflowed = queue.len() >= MAX_QUEUED_FRAMES;
		if overflowed {
			queue.pop_front();
		}
		queue.push_back(frame);
		drop(queue);
		self.outgoing_ready.notify_one();
		!overflowed
	}

	pub fn pop(&self) -> Option<OutgoingFrame> {
		self.outgoing.lock().pop_front()
	}

	pub fn take_keyframe_request(&self) -> bool {
		self.keyframe_wanted.swap(false, Ordering::AcqRel)
	}

	pub fn report_decode_failure(&self, ssrc: u32) {
		self.decode_failures.lock().push(ssrc);
	}
}
