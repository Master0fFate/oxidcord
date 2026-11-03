use std::sync::Arc;
use std::time::Duration;

use protocol::{DispatchEvent, Message, Nonce, Ready, ReadySupplemental, Snowflake, StatusKind, User};

use crate::loaded::Loaded;
use crate::messages::PendingFile;
use crate::notifications::AlertVerdict;
use crate::plugin_shell::{ChannelDecoration, ClientOptions, DmSectionView, UiIntent};
use crate::views::{RenderedMessage, UserOverride};
use crate::voice::{E2eeInfo, StreamInfo, VideoKind, VoiceStatus};

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
		files: Vec<PendingFile>,
		author: Box<User>,
		by_plugin: Option<String>,
		silent: bool,
	},
	PendingFailed {
		nonce: Nonce,
		reason: Option<String>,
		retryable: bool,
	},
	PendingRetried {
		nonce: Nonce,
	},
	UploadProgress {
		nonce: Nonce,
		uploaded: u64,
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
	SlowmodeStarted {
		channel_id: Snowflake,
		wait: Duration,
	},
	DraftChanged {
		channel_id: Snowflake,
		content: String,
	},
	ChannelSelected {
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
	},
	SelectionCleared,
	CategoryCollapsed {
		channel_id: Snowflake,
		collapsed: bool,
	},
	BlockedRunToggled {
		channel_id: Snowflake,
		first: Snowflake,
	},
	ConnectionChanged {
		connected: bool,
	},
	SignedOut,
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
	HandRequested(bool),
	VoiceE2ee(Option<E2eeInfo>),
	VoiceE2eeFailed,
	SettingsEdited,
	RestorePrivacy(Vec<u8>),
	RestoreSensitiveMedia(Vec<u8>),
	CameraChanged(bool),
	ScreenShareChanged(Option<StreamInfo>),
	Watching(Option<String>),
	StreamPreview {
		stream_key: String,
		url: String,
	},
	RemoteVideo {
		user_id: Snowflake,
		kind: VideoKind,
		active: bool,
	},
	Loaded(Box<Loaded>),
	CommandRejected {
		reason: String,
	},
	InteractionStarted {
		nonce: Nonce,
		channel_id: Snowflake,
		message_id: Option<Snowflake>,
	},
	InteractionEnded {
		nonce: Nonce,
	},
	EphemeralDismissed {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	FriendRequestSent {
		username: String,
	},
	FriendRequestFailed {
		username: String,
		reason: String,
	},
	StatusChosen(StatusKind),
	AfkChanged(bool),
	CustomStatusExpired,
	UiIntent(UiIntent),
	ClientOptionsChanged(ClientOptions),
	DmSectionsChanged(Vec<DmSectionView>),
	AlertHold(bool),
	AlertResolved {
		id: u64,
		verdict: AlertVerdict,
	},
	AutocompleteOrder {
		generation: u64,
		order: Vec<u32>,
	},
	ChannelDecorations {
		entries: Vec<(Snowflake, Vec<ChannelDecoration>)>,
	},
	DecorationsCleared,
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
