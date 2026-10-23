use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use parking_lot::Mutex;
use protocol::Snowflake;

use crate::atomic::AtomicF32;
use crate::audio::{AudioEngine, Rings};
use crate::dave::DaveStats;
use crate::denoise;
use crate::event::EventHub;
use crate::video::hub::VideoHub;
use crate::video::{VideoKind, VideoQuality};

const NO_PING: u32 = u32::MAX;
const DEFAULT_VAD_THRESHOLD_DB: f32 = -50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
	#[default]
	VoiceActivity,
	PushToTalk,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VoiceStats {
	pub ping_ms: Option<u32>,
	pub packets_sent: u64,
	pub packets_received: u64,
	pub packets_late: u64,
	pub packets_lost: u64,
	pub jitter_ms: f32,
	pub dave_encrypted: u64,
	pub dave_decrypted: u64,
	pub dave_rejected: u64,
	pub video_sent_frames: u64,
	pub video_received_frames: u64,
	pub video_dropped: u64,
	pub video_bitrate_bps: u32,
	pub e2ee: bool,
}

pub(crate) enum VideoControl {
	Announce { kind: VideoKind, quality: VideoQuality },
	Active(bool),
	Clear,
}

pub(crate) enum Control {
	Speaking(bool),
	Video(VideoControl),
}

pub(crate) struct Counters {
	pub sent: AtomicU64,
	pub received: AtomicU64,
	pub late: AtomicU64,
	pub lost: AtomicU64,
	pub jitter_ms: AtomicF32,
	pub video_sent: AtomicU64,
	pub video_received: AtomicU64,
	pub video_dropped: AtomicU64,
	ping_ms: AtomicU32,
	pub dave: Arc<DaveStats>,
}

impl Counters {
	fn new(dave: Arc<DaveStats>) -> Self {
		Self {
			sent: AtomicU64::new(0),
			received: AtomicU64::new(0),
			late: AtomicU64::new(0),
			lost: AtomicU64::new(0),
			jitter_ms: AtomicF32::new(0.0),
			video_sent: AtomicU64::new(0),
			video_received: AtomicU64::new(0),
			video_dropped: AtomicU64::new(0),
			ping_ms: AtomicU32::new(NO_PING),
			dave,
		}
	}

	pub fn set_ping(&self, ping_ms: u32) {
		self.ping_ms.store(ping_ms, Ordering::Relaxed);
	}

	pub fn snapshot(&self, video_bitrate_bps: u32) -> VoiceStats {
		VoiceStats {
			ping_ms: Some(self.ping_ms.load(Ordering::Relaxed)).filter(|ping| *ping != NO_PING),
			packets_sent: self.sent.load(Ordering::Relaxed),
			packets_received: self.received.load(Ordering::Relaxed),
			packets_late: self.late.load(Ordering::Relaxed),
			packets_lost: self.lost.load(Ordering::Relaxed),
			jitter_ms: self.jitter_ms.load(),
			dave_encrypted: self.dave.encrypted(),
			dave_decrypted: self.dave.decrypted(),
			dave_rejected: self.dave.rejected(),
			video_sent_frames: self.video_sent.load(Ordering::Relaxed),
			video_received_frames: self.video_received.load(Ordering::Relaxed),
			video_dropped: self.video_dropped.load(Ordering::Relaxed),
			video_bitrate_bps,
			e2ee: self.dave.enabled(),
		}
	}
}

pub(crate) struct Shared {
	pub muted: AtomicBool,
	pub deafened: AtomicBool,
	pub transmitting: AtomicBool,
	pub noise_suppression: AtomicBool,
	input_mode: AtomicU8,
	pub vad_threshold_db: AtomicF32,
	pub input_volume: AtomicF32,
	pub output_volume: AtomicF32,
	pub input_level: AtomicF32,
	pub output_level: AtomicF32,
	pub user_volumes: Mutex<HashMap<Snowflake, f32>>,
	pub ssrc_users: Mutex<HashMap<u32, Snowflake>>,
	pub rings: Arc<Rings>,
	pub engine: Mutex<Option<AudioEngine>>,
	pub hub: EventHub,
	pub counters: Counters,
	pub video: VideoHub,
	#[cfg(feature = "testing")]
	pub test_input: Mutex<std::collections::VecDeque<Vec<f32>>>,
	#[cfg(feature = "testing")]
	pub test_output: Mutex<Vec<Vec<i16>>>,
}

impl Shared {
	pub fn new(rings: Arc<Rings>, dave: Arc<DaveStats>, stream_connection: bool) -> Self {
		Self {
			muted: AtomicBool::new(false),
			deafened: AtomicBool::new(false),
			transmitting: AtomicBool::new(false),
			noise_suppression: AtomicBool::new(denoise::AVAILABLE),
			input_mode: AtomicU8::new(InputMode::VoiceActivity as u8),
			vad_threshold_db: AtomicF32::new(DEFAULT_VAD_THRESHOLD_DB),
			input_volume: AtomicF32::new(1.0),
			output_volume: AtomicF32::new(1.0),
			input_level: AtomicF32::new(0.0),
			output_level: AtomicF32::new(0.0),
			user_volumes: Mutex::new(HashMap::new()),
			ssrc_users: Mutex::new(HashMap::new()),
			rings,
			engine: Mutex::new(None),
			hub: EventHub::default(),
			counters: Counters::new(dave),
			video: VideoHub::new(stream_connection),
			#[cfg(feature = "testing")]
			test_input: Mutex::new(std::collections::VecDeque::new()),
			#[cfg(feature = "testing")]
			test_output: Mutex::new(Vec::new()),
		}
	}

	pub fn input_mode(&self) -> InputMode {
		if self.input_mode.load(Ordering::Relaxed) == InputMode::PushToTalk as u8 {
			InputMode::PushToTalk
		} else {
			InputMode::VoiceActivity
		}
	}

	pub fn set_input_mode(&self, mode: InputMode) {
		self.input_mode.store(mode as u8, Ordering::Relaxed);
	}
}
