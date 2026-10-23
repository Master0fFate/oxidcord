use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::Mutex;
use protocol::Snowflake;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::video::VideoKind;

const EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum VoiceEvent {
	Connected {
		ssrc: u32,
	},
	Speaking {
		user_id: Snowflake,
		speaking: bool,
	},
	UserJoined {
		user_id: Snowflake,
	},
	UserLeft {
		user_id: Snowflake,
	},
	Disconnected {
		code: Option<u16>,
		reconnecting: bool,
	},
	Stats {
		ping_ms: u32,
		packets_lost: u64,
		jitter_ms: f32,
	},
	Dave {
		epoch: u64,
		version: u8,
		members: Vec<Snowflake>,
	},
	DaveFailure(String),
	VideoStarted {
		user_id: Snowflake,
		ssrc: u32,
		kind: VideoKind,
	},
	VideoStopped {
		user_id: Snowflake,
		ssrc: u32,
		kind: VideoKind,
	},
}

#[derive(Default)]
pub(crate) struct EventHub {
	subscribers: Mutex<Vec<mpsc::Sender<VoiceEvent>>>,
	ssrc: AtomicU32,
}

impl EventHub {
	pub fn subscribe(&self) -> mpsc::Receiver<VoiceEvent> {
		let (sender, receiver) = mpsc::channel(EVENT_CAPACITY);
		let ssrc = self.ssrc.load(Ordering::Acquire);
		if ssrc != 0 {
			let _ = sender.try_send(VoiceEvent::Connected { ssrc });
		}
		self.subscribers.lock().push(sender);
		receiver
	}

	pub fn emit(&self, event: VoiceEvent) {
		match &event {
			VoiceEvent::Connected { ssrc } => self.ssrc.store(*ssrc, Ordering::Release),
			VoiceEvent::Disconnected {
				reconnecting: false, ..
			} => self.ssrc.store(0, Ordering::Release),
			_ => {}
		}
		self.subscribers
			.lock()
			.retain(|subscriber| !matches!(subscriber.try_send(event.clone()), Err(TrySendError::Closed(_))));
	}

	pub fn ssrc(&self) -> Option<u32> {
		Some(self.ssrc.load(Ordering::Acquire)).filter(|ssrc| *ssrc != 0)
	}
}
