use std::sync::Arc;
use std::time::Duration;

use protocol::{DispatchEvent, Message, Nonce, Ready, ReadySupplemental, Snowflake, StatusKind, User};

use crate::loaded::Loaded;
use crate::views::{RenderedMessage, UserOverride};
use crate::voice::{StreamInfo, VideoKind, VoiceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadDirection {
	History,
	Around,
	Newer,
}

#[derive(Debug, Clone)]
pub enum LocalEvent {
	PendingMessage {
		channel_id: Snowflake,
		nonce: Nonce,
		content: String,
		author: Box<User>,
		by_plugin: Option<String>,
	},
	PendingFailed {
		nonce: Nonce,
		reason: Option<String>,
	},
	PendingAltered {
		nonce: Nonce,
		content: String,
		by: String,
	},
	MessageRendered {
		channel_id: Snowflake,
		id: Snowflake,
		source: Arc<Message>,
		rendered: Arc<RenderedMessage>,
	},
	UserOverrides {
		entries: Vec<(Option<Snowflake>, Snowflake, Option<UserOverride>)>,
	},
	UserOverridesCleared,
	KeepDeleted(bool),
	PendingDiscarded {
		nonce: Nonce,
	},
	DraftChanged {
		channel_id: Snowflake,
		content: String,
	},
	ChannelSelected {
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
	},
	CategoryCollapsed {
		channel_id: Snowflake,
		collapsed: bool,
	},
	ConnectionChanged {
		connected: bool,
	},
	LatencyMeasured(Duration),
	VoiceConnection {
		status: VoiceStatus,
		channel_id: Snowflake,
		guild_id: Option<Snowflake>,
	},
	VoiceSelf {
		self_mute: bool,
		self_deaf: bool,
	},
	Speaking {
		user_id: Snowflake,
		speaking: bool,
	},
	VoiceStats {
		ping_ms: Option<u32>,
		loss: u64,
		jitter: f32,
		e2ee: bool,
		video_bitrate_bps: u32,
	},
	MicTestStarted {
		seconds: u32,
	},
	MicTestLevel {
		level: f32,
		remaining_secs: u32,
	},
	MicTestFinished,
	AccountSwitchRequested(Snowflake),
	VoiceLevels {
		input: f32,
		output: f32,
	},
	CameraChanged(bool),
	ScreenShareChanged(Option<StreamInfo>),
	Watching(Option<String>),
	RemoteVideo {
		user_id: Snowflake,
		kind: VideoKind,
		active: bool,
	},
	Loaded(Box<Loaded>),
	CommandRejected {
		reason: String,
	},
	StatusChosen(StatusKind),
	AfkChanged(bool),
}

#[derive(Debug)]
pub enum Event<'a> {
	Ready(&'a Ready),
	ReadySupplemental(&'a ReadySupplemental),
	Dispatch(&'a DispatchEvent),
	MessagesLoaded {
		channel_id: Snowflake,
		messages: Vec<Message>,
		direction: LoadDirection,
		reached_start: bool,
	},
	Local(LocalEvent),
}
