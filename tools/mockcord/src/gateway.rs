use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use protocol::gateway::capability::AUTH_TOKEN_REFRESH;
use protocol::{GatewayPayload, Opcode, User};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::fixtures::CURRENT_USER_ID;
use crate::hub::{Link, Outgoing};
use crate::json::parse_id;
use crate::member_list::{self, presence_json};
use crate::ready;
use crate::scenario::stream_create_event;
use crate::state::Shared;
use crate::voice::VOICE_TOKEN;

const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(30);
const CLOSE_AUTH_FAILED: u16 = 4004;
const CLOSE_DECODE_ERROR: u16 = 4002;
const CLOSE_NOT_AUTHENTICATED: u16 = 4003;
const CLOSE_ALREADY_AUTHENTICATED: u16 = 4005;
const CLOSE_RATE_LIMITED: u16 = 4008;
const CLOSE_TIMED_OUT: u16 = 4009;
const CLOSE_GOING_AWAY: u16 = 1001;
const FRAME_LIMIT: usize = 120;
const FRAME_WINDOW: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
pub struct GatewayQuery {
	compress: Option<String>,
}

pub async fn upgrade(
	State(shared): State<Arc<Shared>>,
	Query(query): Query<GatewayQuery>,
	socket: WebSocketUpgrade,
) -> Response {
	let compress = query.compress.as_deref() == Some("zlib-stream");
	socket.on_upgrade(move |socket| Connection::new(shared, socket, compress).run())
}

struct Connection {
	shared: Arc<Shared>,
	socket: WebSocket,
	encoder: Option<ZlibEncoder<Vec<u8>>>,
	link_id: u64,
	tx: UnboundedSender<Outgoing>,
	rx: UnboundedReceiver<Outgoing>,
	session: Option<String>,
	received: VecDeque<Instant>,
}

impl Connection {
	fn new(shared: Arc<Shared>, socket: WebSocket, compress: bool) -> Self {
		let (tx, rx) = unbounded_channel();
		let link_id = shared.hub.next_link_id();
		Self {
			shared,
			socket,
			encoder: compress.then(|| ZlibEncoder::new(Vec::new(), Compression::default())),
			link_id,
			tx,
			rx,
			session: None,
			received: VecDeque::with_capacity(FRAME_LIMIT + 1),
		}
	}

	fn over_frame_limit(&mut self) -> bool {
		let now = Instant::now();
		while self
			.received
			.front()
			.is_some_and(|at| now.duration_since(*at) >= FRAME_WINDOW)
		{
			self.received.pop_front();
		}
		self.received.push_back(now);
		self.received.len() > FRAME_LIMIT
	}

	async fn run(mut self) {
		let hello =
			json!({ "op": 10, "d": { "heartbeat_interval": self.shared.config.heartbeat_interval_ms } });
		if !self.send_text(&hello.to_string()).await {
			return;
		}
		let mut shutdown = self.shared.shutdown.clone();
		let identify_deadline = tokio::time::sleep(IDENTIFY_TIMEOUT);
		tokio::pin!(identify_deadline);
		loop {
			tokio::select! {
				incoming = self.socket.recv() => match incoming {
					Some(Ok(message)) => {
						if !self.handle_message(message).await {
							break;
						}
					}
					_ => break,
				},
				outgoing = self.rx.recv() => match outgoing {
					Some(Outgoing::Frame(frame)) => {
						if !self.send_text(&frame).await {
							break;
						}
					}
					Some(Outgoing::Close(code)) => {
						self.close(code, "Closed by mock").await;
						break;
					}
					None => break,
				},
				() = &mut identify_deadline, if self.session.is_none() => {
					self.close(CLOSE_TIMED_OUT, "Session timed out").await;
					break;
				}
				_ = shutdown.changed() => {
					self.close(CLOSE_GOING_AWAY, "Server shutting down").await;
					break;
				}
			}
		}
		if let Some(session) = &self.session {
			self.shared.hub.detach(session, self.link_id);
		}
	}

	async fn send_text(&mut self, text: &str) -> bool {
		let message = match &mut self.encoder {
			Some(encoder) => {
				if encoder
					.write_all(text.as_bytes())
					.and_then(|()| encoder.flush())
					.is_err()
				{
					return false;
				}
				Message::Binary(Bytes::from(std::mem::take(encoder.get_mut())))
			}
			None => Message::Text(text.into()),
		};
		self.socket.send(message).await.is_ok()
	}

	async fn close(&mut self, code: u16, reason: &'static str) {
		let frame = CloseFrame {
			code,
			reason: reason.into(),
		};
		let _ = self.socket.send(Message::Close(Some(frame))).await;
	}

	async fn handle_message(&mut self, message: Message) -> bool {
		match message {
			Message::Text(text) => self.handle_payload(text.as_str()).await,
			Message::Close(_) => false,
			Message::Ping(_) | Message::Pong(_) => true,
			Message::Binary(_) => {
				self.close(CLOSE_DECODE_ERROR, "Binary frames are not supported")
					.await;
				false
			}
		}
	}

	async fn handle_payload(&mut self, text: &str) -> bool {
		if self.over_frame_limit() {
			self.close(CLOSE_RATE_LIMITED, "Rate limited.").await;
			return false;
		}
		let Ok(payload) = serde_json::from_str::<GatewayPayload>(text) else {
			self.close(CLOSE_DECODE_ERROR, "Decode error").await;
			return false;
		};
		let data = payload
			.d
			.as_deref()
			.and_then(|raw| serde_json::from_str::<Value>(raw.get()).ok())
			.unwrap_or(Value::Null);
		match payload.opcode() {
			Opcode::Heartbeat => self.send_text(r#"{"op":11}"#).await,
			Opcode::Identify => self.identify(&data).await,
			Opcode::Resume => self.resume(&data).await,
			Opcode::QosHeartbeat => true,
			other => {
				let Some(session) = self.session.clone() else {
					self.close(CLOSE_NOT_AUTHENTICATED, "Not authenticated").await;
					return false;
				};
				self.handle_session_op(other, &session, &data);
				true
			}
		}
	}

	fn account_for_token(&self, data: &Value) -> Option<User> {
		data["token"]
			.as_str()
			.and_then(|token| self.shared.account_for_token(token))
	}

	async fn identify(&mut self, data: &Value) -> bool {
		let Some(user) = self.account_for_token(data) else {
			self.close(CLOSE_AUTH_FAILED, "Authentication failed.").await;
			return false;
		};
		if self.session.is_some() {
			self.close(CLOSE_ALREADY_AUTHENTICATED, "Already authenticated.")
				.await;
			return false;
		}
		let hub = &self.shared.hub;
		let capabilities = data["capabilities"].as_u64().unwrap_or_default();
		let session_id = hub.create_session(
			Link {
				id: self.link_id,
				tx: self.tx.clone(),
			},
			capabilities,
		);
		let (mut ready, supplemental) = ready::build(&self.shared, &session_id, &user, capabilities);
		if capabilities & AUTH_TOKEN_REFRESH != 0 {
			let token = data["token"].as_str().unwrap_or_default();
			ready["auth_token"] = json!(self.shared.config.rotated_token(token));
		}
		hub.dispatch_to(&session_id, "READY", &ready);
		hub.dispatch_to(&session_id, "READY_SUPPLEMENTAL", &supplemental);
		self.session = Some(session_id);
		true
	}

	async fn resume(&mut self, data: &Value) -> bool {
		if self.account_for_token(data).is_none() {
			self.close(CLOSE_AUTH_FAILED, "Authentication failed.").await;
			return false;
		}
		let (Some(session_id), Some(seq)) = (data["session_id"].as_str(), data["seq"].as_u64()) else {
			self.close(CLOSE_DECODE_ERROR, "Decode error").await;
			return false;
		};
		let link = Link {
			id: self.link_id,
			tx: self.tx.clone(),
		};
		if self.shared.hub.resume(session_id, seq, link) {
			self.session = Some(session_id.to_owned());
			true
		} else {
			self.send_text(r#"{"op":9,"d":false}"#).await
		}
	}

	fn handle_session_op(&self, opcode: Opcode, session: &str, data: &Value) {
		let hub = &self.shared.hub;
		match opcode {
			Opcode::GuildSubscriptionsBulk => {
				for guild_id in member_list::bulk_guild_ids(data) {
					hub.subscribe(session, guild_id);
				}
				for update in member_list::bulk_updates(&self.shared, data) {
					hub.dispatch_to(session, "GUILD_MEMBER_LIST_UPDATE", &update);
				}
			}
			Opcode::GuildSubscriptions => {
				if let Some(guild_id) = parse_id(&data["guild_id"]) {
					hub.subscribe(session, guild_id);
				}
				for update in member_list::legacy_updates(&self.shared, data) {
					hub.dispatch_to(session, "GUILD_MEMBER_LIST_UPDATE", &update);
				}
			}
			Opcode::RequestGuildMembers => {
				for chunk in member_list::members_chunks(&self.shared, data) {
					hub.dispatch_to(session, "GUILD_MEMBERS_CHUNK", &chunk);
				}
			}
			Opcode::RequestChannelInfo => {
				if let Some(info) = self.shared.channel_info(data) {
					hub.dispatch_to(session, "CHANNEL_INFO", &info);
				}
			}
			Opcode::PresenceUpdate => self.broadcast_presence(data),
			Opcode::VoiceStateUpdate => self.broadcast_voice_state(session, data),
			Opcode::StreamCreate => self.stream_create(session, data),
			Opcode::StreamDelete => self.stream_delete(session, data),
			Opcode::StreamWatch => self.stream_watch(session, data),
			Opcode::StreamSetPaused => self.stream_paused(session, data),
			_ => {}
		}
	}

	fn broadcast_presence(&self, data: &Value) {
		let status = data["status"].as_str().unwrap_or("online");
		let activities = data
			.get("activities")
			.filter(|value| value.is_array())
			.cloned()
			.unwrap_or(json!([]));
		for guild in &self.shared.fixtures.guilds {
			let mut event = presence_json(guild.id, CURRENT_USER_ID, &Default::default());
			event["status"] = json!(status);
			event["activities"] = activities.clone();
			event["client_status"] = json!({ "desktop": status });
			self.shared.hub.broadcast("PRESENCE_UPDATE", &event);
		}
	}

	fn stream_key(data: &Value) -> Option<(String, Option<u64>, u64)> {
		let channel_id = parse_id(&data["channel_id"])?;
		let guild_id = parse_id(&data["guild_id"]);
		let key = match guild_id {
			Some(guild_id) => format!("guild:{guild_id}:{channel_id}:{CURRENT_USER_ID}"),
			None => format!("call:{channel_id}:{CURRENT_USER_ID}"),
		};
		Some((key, guild_id, channel_id))
	}

	fn announce_stream(
		&self,
		session: &str,
		key: &str,
		guild_id: Option<u64>,
		channel_id: u64,
		viewers: &[u64],
	) {
		let hub = &self.shared.hub;
		hub.dispatch_to(
			session,
			"STREAM_CREATE",
			&stream_create_event(key, channel_id, viewers),
		);
		hub.dispatch_to(
			session,
			"STREAM_SERVER_UPDATE",
			&json!({
				"token": VOICE_TOKEN,
				"stream_key": key,
				"guild_id": guild_id.map(|id| id.to_string()),
				"endpoint": self.shared.urls.voice,
			}),
		);
	}

	fn stream_create(&self, session: &str, data: &Value) {
		if let Some((key, guild_id, channel_id)) = Self::stream_key(data) {
			self.announce_stream(session, &key, guild_id, channel_id, &[]);
		}
	}

	fn stream_watch(&self, session: &str, data: &Value) {
		let Some(key) = data["stream_key"].as_str() else {
			return;
		};
		let parts: Vec<u64> = key
			.split(':')
			.skip(1)
			.filter_map(|part| part.parse().ok())
			.collect();
		let (guild_id, channel_id) = match parts.as_slice() {
			[guild_id, channel_id, _] => (Some(*guild_id), *channel_id),
			[channel_id, _] => (None, *channel_id),
			_ => return,
		};
		self.announce_stream(session, key, guild_id, channel_id, &[CURRENT_USER_ID]);
	}

	fn stream_delete(&self, session: &str, data: &Value) {
		if let Some(key) = data["stream_key"].as_str() {
			self.shared.hub.dispatch_to(
				session,
				"STREAM_DELETE",
				&json!({ "stream_key": key, "reason": "user_requested" }),
			);
		}
	}

	fn stream_paused(&self, session: &str, data: &Value) {
		if let Some(key) = data["stream_key"].as_str() {
			self.shared.hub.dispatch_to(
				session,
				"STREAM_UPDATE",
				&json!({
					"stream_key": key,
					"region": "mock",
					"viewer_ids": [],
					"paused": data["paused"].as_bool().unwrap_or(false),
				}),
			);
		}
	}

	fn broadcast_voice_state(&self, session: &str, data: &Value) {
		let guild_id = parse_id(&data["guild_id"]);
		let channel_id = parse_id(&data["channel_id"]);
		let changed = self.shared.remember_voice(session, guild_id, channel_id)
			| self.shared.voice_stale.swap(false, Ordering::AcqRel);
		self.shared.sync_seat(changed, channel_id, data);
		let event = self.shared.voice_state_event(session, guild_id, channel_id);
		self.shared.hub.broadcast("VOICE_STATE_UPDATE", &event);
		if let (true, Some(channel_id)) = (changed, channel_id) {
			let server = self.shared.voice_server_event(guild_id, channel_id);
			self.shared
				.hub
				.dispatch_to(session, "VOICE_SERVER_UPDATE", &server);
			if let Some(stream) = self.shared.occupant_stream(guild_id, channel_id) {
				self.shared.hub.dispatch_to(session, "STREAM_CREATE", &stream);
			}
		}
	}
}
