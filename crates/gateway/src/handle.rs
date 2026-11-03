use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use protocol::Snowflake;
use protocol::gateway::Opcode;
use serde_json::{Value, json};
use tokio::runtime::Handle;
use tokio::sync::{Notify, mpsc, watch};

use crate::config::GatewayConfig;
use crate::error::SendError;
use crate::event::GatewayEvent;
use crate::frames;
use crate::outbox::Queued;
use crate::session::Session;
use crate::subscription::GuildSubscription;

const EVENT_CAPACITY: usize = 1024;
const COMMAND_CAPACITY: usize = 256;
pub(crate) const NO_LATENCY: u64 = u64::MAX;

pub struct Gateway;

impl Gateway {
	pub fn spawn(config: GatewayConfig, handle: Handle) -> (GatewayHandle, mpsc::Receiver<GatewayEvent>) {
		let (events, event_receiver) = mpsc::channel(EVENT_CAPACITY);
		let (commands, command_receiver) = mpsc::channel(COMMAND_CAPACITY);
		let (shutdown, shutdown_receiver) = watch::channel(false);
		let gateway_handle = GatewayHandle {
			commands,
			reconnect: Arc::new(Notify::new()),
			shutdown: Arc::new(shutdown),
			latency: Arc::new(AtomicU64::new(NO_LATENCY)),
		};
		let session = Session::new(
			config,
			events,
			command_receiver,
			gateway_handle.reconnect.clone(),
			shutdown_receiver,
			gateway_handle.latency.clone(),
		);
		handle.spawn(session.run());
		(gateway_handle, event_receiver)
	}
}

#[derive(Clone)]
pub struct GatewayHandle {
	commands: mpsc::Sender<Queued>,
	reconnect: Arc<Notify>,
	shutdown: Arc<watch::Sender<bool>>,
	latency: Arc<AtomicU64>,
}

impl GatewayHandle {
	pub fn send_presence(&self, status: &str, activities: Vec<Value>, afk: bool) -> Result<(), SendError> {
		self.queue(
			Opcode::PresenceUpdate,
			json!({ "status": status, "since": 0, "activities": activities, "afk": afk }),
		)
	}

	pub fn subscribe_guild(
		&self,
		guild_id: Snowflake,
		channels: Vec<(Snowflake, Vec<[u32; 2]>)>,
		typing: bool,
		threads: bool,
		activities: bool,
	) -> Result<(), SendError> {
		self.enqueue(Queued::Subscribe {
			guild_id,
			subscription: GuildSubscription {
				typing,
				threads,
				activities,
				channels: channels.into_iter().collect(),
			},
		})
	}

	pub fn request_members(
		&self,
		guild_id: Snowflake,
		query: &str,
		limit: u32,
		user_ids: Vec<Snowflake>,
	) -> Result<(), SendError> {
		let data = if user_ids.is_empty() {
			json!({ "guild_id": guild_id, "query": query, "limit": limit })
		} else {
			json!({ "guild_id": guild_id, "user_ids": user_ids, "limit": limit, "presences": true })
		};
		self.queue(Opcode::RequestGuildMembers, data)
	}

	pub fn request_channel_info(&self, guild_id: Snowflake) -> Result<(), SendError> {
		self.queue(
			Opcode::RequestChannelInfo,
			json!({ "guild_id": guild_id, "fields": ["status", "voice_start_time"] }),
		)
	}

	pub fn request_soundboard_sounds(&self, guild_ids: Vec<Snowflake>) -> Result<(), SendError> {
		self.queue(Opcode::RequestSoundboardSounds, json!({ "guild_ids": guild_ids }))
	}

	pub fn voice_state(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		self_mute: bool,
		self_deaf: bool,
	) -> Result<(), SendError> {
		self.voice_state_video(guild_id, channel_id, self_mute, self_deaf, false)
	}

	pub fn voice_state_video(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		self_mute: bool,
		self_deaf: bool,
		self_video: bool,
	) -> Result<(), SendError> {
		self.queue(
			Opcode::VoiceStateUpdate,
			json!({
				"guild_id": guild_id,
				"channel_id": channel_id,
				"self_mute": self_mute,
				"self_deaf": self_deaf,
				"self_video": self_video,
			}),
		)
	}

	pub fn stream_create(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
		preferred_region: Option<&str>,
	) -> Result<(), SendError> {
		let kind = if guild_id.is_some() { "guild" } else { "call" };
		self.queue(
			Opcode::StreamCreate,
			json!({
				"type": kind,
				"guild_id": guild_id,
				"channel_id": channel_id,
				"preferred_region": preferred_region,
			}),
		)
	}

	pub fn stream_delete(&self, stream_key: &str) -> Result<(), SendError> {
		self.queue(Opcode::StreamDelete, json!({ "stream_key": stream_key }))
	}

	pub fn stream_watch(&self, stream_key: &str) -> Result<(), SendError> {
		self.queue(Opcode::StreamWatch, json!({ "stream_key": stream_key }))
	}

	pub fn stream_set_paused(&self, stream_key: &str, paused: bool) -> Result<(), SendError> {
		self.queue(
			Opcode::StreamSetPaused,
			json!({ "stream_key": stream_key, "paused": paused }),
		)
	}

	pub fn reconnect(&self) {
		self.reconnect.notify_one();
	}

	pub fn shutdown(&self) {
		self.shutdown.send_replace(true);
	}

	pub fn latency(&self) -> Option<Duration> {
		match self.latency.load(Ordering::Relaxed) {
			NO_LATENCY => None,
			micros => Some(Duration::from_micros(micros)),
		}
	}

	fn queue(&self, opcode: Opcode, data: Value) -> Result<(), SendError> {
		self.enqueue(Queued::Frame(frames::command(opcode, data)))
	}

	fn enqueue(&self, queued: Queued) -> Result<(), SendError> {
		self.commands.try_send(queued).map_err(|error| match error {
			mpsc::error::TrySendError::Full(_) => SendError::Full,
			mpsc::error::TrySendError::Closed(_) => SendError::Closed,
		})
	}
}
