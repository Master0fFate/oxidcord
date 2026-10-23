use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use protocol::Snowflake;
use protocol::gateway::Opcode;
use serde_json::{Map, Value, json};
use tokio::runtime::Handle;
use tokio::sync::{Notify, mpsc, watch};

use crate::config::GatewayConfig;
use crate::error::SendError;
use crate::event::GatewayEvent;
use crate::frames;
use crate::session::Session;

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
	commands: mpsc::Sender<String>,
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
		let channels: Map<String, Value> = channels
			.into_iter()
			.map(|(channel_id, ranges)| (channel_id.to_string(), json!(ranges)))
			.collect();
		let subscription = json!({
			"typing": typing,
			"threads": threads,
			"activities": activities,
			"members": [],
			"channels": channels,
		});
		self.queue(
			Opcode::GuildSubscriptionsBulk,
			json!({ "subscriptions": { guild_id.to_string(): subscription } }),
		)
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
			json!({ "guild_id": guild_id, "user_ids": user_ids, "limit": limit })
		};
		self.queue(Opcode::RequestGuildMembers, data)
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

	pub fn voice_state_request_to_speak(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		self_mute: bool,
		self_deaf: bool,
		request_to_speak_timestamp: Option<DateTime<Utc>>,
	) -> Result<(), SendError> {
		self.queue(
			Opcode::VoiceStateUpdate,
			json!({
				"guild_id": guild_id,
				"channel_id": channel_id,
				"self_mute": self_mute,
				"self_deaf": self_deaf,
				"self_video": false,
				"request_to_speak_timestamp": request_to_speak_timestamp.map(|time| time.to_rfc3339()),
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
		self.commands
			.try_send(frames::command(opcode, data))
			.map_err(|error| match error {
				mpsc::error::TrySendError::Full(_) => SendError::Full,
				mpsc::error::TrySendError::Closed(_) => SendError::Closed,
			})
	}
}
