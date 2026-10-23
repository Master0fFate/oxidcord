use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use protocol::{Snowflake, Token};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Interval, MissedTickBehavior, interval_at, sleep, timeout};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};
use tracing::{debug, warn};
use url::{Host, Url};

use crate::crypto::{EncryptionMode, PacketCipher};
use crate::dave::{self, DaveEvent, DaveSession, Outgoing};
use crate::error::{Result, VoiceError};
use crate::event::VoiceEvent;
use crate::media::{Media, SharedTransform, Transport};
use crate::shared::{Control, Shared, VideoControl};
use crate::video::rtp as video_rtp;
use crate::video::{VideoKind, VideoQuality, VideoStream};
use crate::wire::{
	self, ClientDisconnect, ClientsConnect, HeartbeatAck, Hello, RawFrame, Ready, SessionDescription,
	Speaking, VideoAnnouncement, opcode,
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(1);
const DISCOVERY_ATTEMPTS: usize = 3;
const DISCOVERY_PACKET_BYTES: usize = 74;
const DISCOVERY_LENGTH_FIELD: u16 = 70;
const DISCOVERY_REQUEST: u16 = 1;
const DISCOVERY_RESPONSE: u16 = 2;
const ADDRESS_RANGE: std::ops::Range<usize> = 8..72;
const RESUME_ATTEMPTS: u32 = 5;
const FRESH_ATTEMPTS: u32 = 3;
const MAX_BACKOFF_SHIFT: u32 = 3;
const NORMAL_CLOSE_REASON: &str = "";
const MAX_BACKLOG_FRAMES: usize = 64;
const BINARY_HEADER_BYTES: usize = 3;
const OPUS_PAYLOAD_TYPE: u8 = 120;
const CODEC_PRIORITY: u32 = 1000;
const FULL_QUALITY: u32 = 100;
const MAX_VIDEO_RID: &str = "100";

pub(crate) struct Credentials {
	server_id: Snowflake,
	user_id: Snowflake,
	session_id: String,
	token: Token,
	url: Url,
	stream: bool,
}

impl Credentials {
	pub fn new(
		server_id: Snowflake,
		user_id: Snowflake,
		session_id: String,
		token: Token,
		endpoint: &str,
		stream: bool,
	) -> Result<Self> {
		Ok(Self {
			server_id,
			user_id,
			session_id,
			token,
			url: endpoint_url(endpoint)?,
			stream,
		})
	}

	pub fn user_id(&self) -> Snowflake {
		self.user_id
	}

	fn identify(&self) -> Value {
		json!({
			"op": opcode::IDENTIFY,
			"d": {
				"server_id": self.server_id,
				"user_id": self.user_id,
				"session_id": self.session_id,
				"token": self.token.as_str(),
				"max_dave_protocol_version": dave::PROTOCOL_VERSION,
				"video": true,
				"streams": [{
					"type": if self.stream { "screen" } else { "video" },
					"rid": MAX_VIDEO_RID,
					"quality": FULL_QUALITY,
					"active": false,
				}],
			},
		})
	}

	fn resume(&self, seq_ack: i64) -> Value {
		json!({
			"op": opcode::RESUME,
			"d": {
				"server_id": self.server_id,
				"session_id": self.session_id,
				"token": self.token.as_str(),
				"seq_ack": seq_ack,
			},
		})
	}
}

fn endpoint_url(endpoint: &str) -> Result<Url> {
	let raw = if endpoint.contains("://") {
		endpoint.to_owned()
	} else {
		format!("wss://{endpoint}")
	};
	let mut url = Url::parse(&raw).map_err(|_| VoiceError::InvalidEndpoint)?;
	let secure = match url.scheme() {
		"wss" => true,
		"ws" => is_loopback(&url),
		_ => false,
	};
	if !secure {
		return Err(VoiceError::InsecureEndpoint);
	}
	url.set_path("/");
	url.query_pairs_mut()
		.clear()
		.append_pair("v", wire::GATEWAY_VERSION);
	Ok(url)
}

fn is_loopback(url: &Url) -> bool {
	match url.host() {
		Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
		Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
		Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
		None => false,
	}
}

#[derive(Debug)]
pub(crate) enum Exit {
	Closed(Option<u16>),
	Lost,
}

fn exit_error(exit: Exit) -> VoiceError {
	match exit {
		Exit::Closed(code) => VoiceError::Closed(code.unwrap_or(0)),
		Exit::Lost => VoiceError::Transport("connection lost".to_owned()),
	}
}

struct Frame {
	op: u8,
	body: Body,
}

enum Body {
	Json(Value),
	Binary(Bytes),
}

struct Heartbeat {
	timer: Interval,
	nonce: u64,
	sent_at: Instant,
	awaiting: bool,
}

pub(crate) struct Link {
	socket: Socket,
	heartbeat: Option<Heartbeat>,
	last_seq: i64,
	ping_ms: Option<u32>,
	backlog: VecDeque<Frame>,
}

impl Link {
	async fn connect(url: &Url) -> Result<Self> {
		let config = WebSocketConfig::default()
			.max_frame_size(Some(MAX_FRAME_BYTES))
			.max_message_size(Some(MAX_FRAME_BYTES));
		let (socket, _) = timeout(
			CONNECT_TIMEOUT,
			connect_async_with_config(url.as_str(), Some(config), true),
		)
		.await
		.map_err(|_| VoiceError::Timeout)?
		.map_err(|error| VoiceError::Transport(error.to_string()))?;
		Ok(Self {
			socket,
			heartbeat: None,
			last_seq: -1,
			ping_ms: None,
			backlog: VecDeque::new(),
		})
	}

	async fn send(&mut self, frame: Value) -> std::result::Result<(), Exit> {
		self.send_message(Message::text(frame.to_string())).await
	}

	async fn send_message(&mut self, message: Message) -> std::result::Result<(), Exit> {
		self.socket.send(message).await.map_err(|_| Exit::Lost)
	}

	async fn close(&mut self) {
		let frame = CloseFrame {
			code: CloseCode::Normal,
			reason: NORMAL_CLOSE_REASON.into(),
		};
		let _ = timeout(CLOSE_TIMEOUT, self.socket.close(Some(frame))).await;
	}

	async fn next(&mut self) -> std::result::Result<Frame, Exit> {
		match self.backlog.pop_front() {
			Some(frame) => Ok(frame),
			None => self.read().await,
		}
	}

	async fn read(&mut self) -> std::result::Result<Frame, Exit> {
		loop {
			tokio::select! {
				message = self.socket.next() => match message {
					Some(Ok(message)) => {
						if let Some(frame) = self.on_message(message)? {
							return Ok(frame);
						}
					}
					Some(Err(error)) => {
						debug!(%error, "voice websocket failed");
						return Err(Exit::Lost);
					}
					None => return Err(Exit::Lost),
				},
				() = next_beat(&mut self.heartbeat) => self.beat().await?,
			}
		}
	}

	async fn expect<T: DeserializeOwned>(&mut self, op: u8) -> Result<T> {
		loop {
			let frame = self.read().await.map_err(exit_error)?;
			if frame.op != op {
				self.defer(frame);
				continue;
			}
			let Body::Json(data) = frame.body else {
				continue;
			};
			return serde_json::from_value(data)
				.map_err(|_| VoiceError::Handshake("malformed voice gateway payload"));
		}
	}

	fn defer(&mut self, frame: Frame) {
		let replayable = matches!(
			frame.op,
			opcode::SPEAKING
				| opcode::CLIENTS_CONNECT
				| opcode::VIDEO
				| opcode::CLIENT_DISCONNECT
				| opcode::DAVE_PREPARE_TRANSITION..=opcode::DAVE_MLS_INVALID_COMMIT_WELCOME
		);
		if replayable && self.backlog.len() < MAX_BACKLOG_FRAMES {
			self.backlog.push_back(frame);
		}
	}

	fn on_binary(&mut self, bytes: &Bytes) -> Option<Frame> {
		let (sequence, rest) = bytes.split_first_chunk::<2>()?;
		self.last_seq = i64::from(u16::from_be_bytes(*sequence));
		Some(Frame {
			op: *rest.first()?,
			body: Body::Binary(bytes.slice(BINARY_HEADER_BYTES..)),
		})
	}

	fn on_message(&mut self, message: Message) -> std::result::Result<Option<Frame>, Exit> {
		let text = match message {
			Message::Text(text) => text,
			Message::Binary(bytes) => return Ok(self.on_binary(&bytes)),
			Message::Close(frame) => return Err(Exit::Closed(frame.map(|frame| u16::from(frame.code)))),
			_ => return Ok(None),
		};
		let Ok(raw) = serde_json::from_str::<RawFrame>(&text) else {
			return Ok(None);
		};
		if let Some(seq) = raw.seq {
			self.last_seq = seq;
		}
		match raw.op {
			opcode::HELLO => {
				if let Ok(hello) = serde_json::from_value::<Hello>(raw.d) {
					self.start_heartbeat(hello.heartbeat_interval);
				}
				Ok(None)
			}
			op => {
				if op == opcode::HEARTBEAT_ACK {
					self.on_ack(&raw.d);
				}
				Ok(Some(Frame {
					op,
					body: Body::Json(raw.d),
				}))
			}
		}
	}

	fn start_heartbeat(&mut self, interval_ms: f64) {
		let period = Duration::from_millis(interval_ms.max(1.0) as u64);
		let first = tokio::time::Instant::now() + period.mul_f32(rand::random::<f32>());
		let mut timer = interval_at(first, period);
		timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
		self.heartbeat = Some(Heartbeat {
			timer,
			nonce: 0,
			sent_at: Instant::now(),
			awaiting: false,
		});
	}

	fn on_ack(&mut self, data: &Value) {
		let Some(heartbeat) = self.heartbeat.as_mut() else {
			return;
		};
		let Ok(ack) = serde_json::from_value::<HeartbeatAck>(data.clone()) else {
			return;
		};
		if ack.t == heartbeat.nonce {
			heartbeat.awaiting = false;
			self.ping_ms = Some(heartbeat.sent_at.elapsed().as_millis() as u32);
		}
	}

	async fn beat(&mut self) -> std::result::Result<(), Exit> {
		let seq_ack = self.last_seq;
		let Some(heartbeat) = self.heartbeat.as_mut() else {
			return Ok(());
		};
		if heartbeat.awaiting {
			return Err(Exit::Lost);
		}
		heartbeat.nonce = unix_millis();
		heartbeat.sent_at = Instant::now();
		heartbeat.awaiting = true;
		let frame = json!({
			"op": opcode::HEARTBEAT,
			"d": { "t": heartbeat.nonce, "seq_ack": seq_ack },
		});
		self.send(frame).await
	}
}

async fn next_beat(heartbeat: &mut Option<Heartbeat>) {
	match heartbeat {
		Some(heartbeat) => {
			heartbeat.timer.tick().await;
		}
		None => std::future::pending().await,
	}
}

fn unix_millis() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VideoSsrcs {
	pub audio: u32,
	pub video: u32,
	pub rtx: u32,
}

impl VideoSsrcs {
	fn from_ready(ready: &Ready) -> Self {
		let assigned = ready.streams.iter().find(|stream| stream.ssrc != 0);
		let (video, rtx) = match assigned {
			Some(stream) => (
				stream.ssrc,
				if stream.rtx_ssrc != 0 {
					stream.rtx_ssrc
				} else {
					stream.ssrc.wrapping_add(1)
				},
			),
			None => (ready.ssrc.wrapping_add(1), ready.ssrc.wrapping_add(2)),
		};
		Self {
			audio: ready.ssrc,
			video,
			rtx,
		}
	}
}

pub(crate) struct Established {
	link: Link,
	transport: Transport,
	dave_version: u8,
	ssrcs: VideoSsrcs,
	keyframe_interval_ms: u32,
}

pub(crate) async fn establish(credentials: &Credentials) -> Result<Established> {
	timeout(HANDSHAKE_TIMEOUT, handshake(credentials))
		.await
		.map_err(|_| VoiceError::Timeout)?
}

async fn handshake(credentials: &Credentials) -> Result<Established> {
	let mut link = Link::connect(&credentials.url).await?;
	link.send(credentials.identify()).await.map_err(exit_error)?;
	let ready: Ready = link.expect(opcode::READY).await?;
	let mode = EncryptionMode::select(&ready.modes).ok_or(VoiceError::NoEncryptionMode)?;
	let ssrcs = VideoSsrcs::from_ready(&ready);
	let media_address: IpAddr = ready
		.ip
		.parse()
		.map_err(|_| VoiceError::Handshake("invalid media server address"))?;
	let socket = UdpSocket::bind(if media_address.is_ipv4() {
		"0.0.0.0:0"
	} else {
		"[::]:0"
	})
	.await?;
	socket.connect((media_address, ready.port)).await?;
	let (address, port) = discover(&socket, ready.ssrc).await?;
	link.send(json!({
		"op": opcode::SELECT_PROTOCOL,
		"d": {
			"protocol": "udp",
			"data": { "address": address, "port": port, "mode": mode.name() },
			"codecs": [
				{
					"name": "opus",
					"type": "audio",
					"priority": CODEC_PRIORITY,
					"payload_type": OPUS_PAYLOAD_TYPE,
				},
				{
					"name": "H264",
					"type": "video",
					"priority": CODEC_PRIORITY,
					"payload_type": video_rtp::PAYLOAD_TYPE,
					"rtx_payload_type": video_rtp::RTX_PAYLOAD_TYPE,
					"encode": true,
					"decode": true,
				},
			],
		},
	}))
	.await
	.map_err(exit_error)?;
	let description: SessionDescription = link.expect(opcode::SESSION_DESCRIPTION).await?;
	let mode = EncryptionMode::from_name(&description.mode).ok_or(VoiceError::NoEncryptionMode)?;
	let cipher = PacketCipher::new(mode, &description.secret_key)?;
	link.send(speaking_frame(false, ready.ssrc))
		.await
		.map_err(exit_error)?;
	link.send(sink_wants_frame()).await.map_err(exit_error)?;
	Ok(Established {
		link,
		transport: Transport {
			socket: Arc::new(socket),
			ssrc: ready.ssrc,
			cipher,
		},
		dave_version: description.dave_protocol_version,
		ssrcs,
		keyframe_interval_ms: description.keyframe_interval.unwrap_or(0),
	})
}

async fn discover(socket: &UdpSocket, ssrc: u32) -> Result<(String, u16)> {
	let mut request = [0u8; DISCOVERY_PACKET_BYTES];
	request[..2].copy_from_slice(&DISCOVERY_REQUEST.to_be_bytes());
	request[2..4].copy_from_slice(&DISCOVERY_LENGTH_FIELD.to_be_bytes());
	request[4..8].copy_from_slice(&ssrc.to_be_bytes());
	let mut response = [0u8; DISCOVERY_PACKET_BYTES];
	for _ in 0..DISCOVERY_ATTEMPTS {
		socket.send(&request).await?;
		let Ok(Ok(length)) = timeout(DISCOVERY_TIMEOUT, socket.recv(&mut response)).await else {
			continue;
		};
		if length < DISCOVERY_PACKET_BYTES
			|| u16::from_be_bytes([response[0], response[1]]) != DISCOVERY_RESPONSE
		{
			continue;
		}
		let text = &response[ADDRESS_RANGE];
		let end = text.iter().position(|byte| *byte == 0).unwrap_or(text.len());
		let address = String::from_utf8_lossy(&text[..end]).into_owned();
		let port = u16::from_be_bytes([response[72], response[73]]);
		return Ok((address, port));
	}
	Err(VoiceError::IpDiscovery)
}

async fn resume(credentials: &Credentials, last_seq: i64) -> Result<Link> {
	let attempt = async {
		let mut link = Link::connect(&credentials.url).await?;
		link.last_seq = last_seq;
		link.send(credentials.resume(last_seq))
			.await
			.map_err(exit_error)?;
		loop {
			let frame = link.next().await.map_err(exit_error)?;
			if frame.op == opcode::RESUMED {
				return Ok(link);
			}
		}
	};
	timeout(HANDSHAKE_TIMEOUT, attempt)
		.await
		.map_err(|_| VoiceError::Timeout)?
}

fn speaking_frame(speaking: bool, ssrc: u32) -> Value {
	json!({
		"op": opcode::SPEAKING,
		"d": { "speaking": u8::from(speaking), "delay": 0, "ssrc": ssrc },
	})
}

fn sink_wants_frame() -> Value {
	json!({ "op": opcode::MEDIA_SINK_WANTS, "d": { "any": FULL_QUALITY } })
}

fn video_frame(ssrcs: VideoSsrcs, active: bool, quality: &VideoQuality) -> Value {
	json!({
		"op": opcode::VIDEO,
		"d": {
			"audio_ssrc": ssrcs.audio,
			"video_ssrc": ssrcs.video,
			"rtx_ssrc": ssrcs.rtx,
			"streams": [{
				"type": "video",
				"rid": MAX_VIDEO_RID,
				"ssrc": ssrcs.video,
				"rtx_ssrc": ssrcs.rtx,
				"quality": FULL_QUALITY,
				"active": active,
				"max_bitrate": quality.bitrate,
				"max_framerate": quality.fps,
				"max_resolution": {
					"type": "fixed",
					"width": quality.width,
					"height": quality.height,
				},
			}],
		},
	})
}

fn clear_video_frame(audio_ssrc: u32) -> Value {
	json!({
		"op": opcode::VIDEO,
		"d": { "audio_ssrc": audio_ssrc, "video_ssrc": 0, "rtx_ssrc": 0, "streams": [] },
	})
}

enum Next {
	Resume,
	Fresh,
	Stop,
}

fn classify(code: Option<u16>) -> Next {
	match code {
		None | Some(wire::close::SERVER_CRASHED) => Next::Resume,
		Some(wire::close::SESSION_INVALID | wire::close::SESSION_TIMEOUT) => Next::Fresh,
		Some(code) if code < 4000 => Next::Resume,
		Some(_) => Next::Stop,
	}
}

fn backoff(attempt: u32) -> Duration {
	Duration::from_secs(1 << attempt.min(MAX_BACKOFF_SHIFT))
}

struct MediaHandle(JoinHandle<()>);

impl Drop for MediaHandle {
	fn drop(&mut self) {
		self.0.abort();
	}
}

enum Drive {
	Shutdown,
	Exit(Exit),
}

enum Recovery {
	Recovered,
	Stopped(Option<u16>),
	Shutdown,
}

pub(crate) struct Supervisor {
	credentials: Credentials,
	shared: Arc<Shared>,
	transform: SharedTransform,
	audio_io: bool,
	control: mpsc::UnboundedSender<Control>,
	commands: mpsc::UnboundedReceiver<Control>,
	shutdown: watch::Receiver<bool>,
	link: Link,
	media: MediaHandle,
	ssrc: u32,
	ssrcs: VideoSsrcs,
	announced: Option<(VideoKind, VideoQuality)>,
	reannounce: bool,
	speaking: bool,
	dave: DaveSession,
	outbox: Vec<Outgoing>,
}

pub(crate) struct Signals {
	pub shutdown: watch::Receiver<bool>,
	pub control: mpsc::UnboundedSender<Control>,
	pub commands: mpsc::UnboundedReceiver<Control>,
}

impl Supervisor {
	pub fn start(
		credentials: Credentials,
		established: Established,
		shared: Arc<Shared>,
		transform: SharedTransform,
		dave: DaveSession,
		audio_io: bool,
		signals: Signals,
	) -> Result<Self> {
		let Signals {
			shutdown,
			control,
			commands,
		} = signals;
		let ssrc = established.transport.ssrc;
		shared
			.video
			.keyframe_interval_ms
			.store(established.keyframe_interval_ms, Ordering::Relaxed);
		let media = spawn_media(established.transport, &shared, &control, &transform, audio_io)?;
		shared.hub.emit(VoiceEvent::Connected { ssrc });
		let mut supervisor = Self {
			credentials,
			shared,
			transform,
			audio_io,
			control,
			commands,
			shutdown,
			link: established.link,
			media,
			ssrc,
			ssrcs: established.ssrcs,
			announced: None,
			reannounce: false,
			speaking: false,
			dave,
			outbox: Vec::new(),
		};
		supervisor.outbox = supervisor.dave.on_protocol_version(established.dave_version);
		while let Some(frame) = supervisor.link.backlog.pop_front() {
			supervisor.on_frame(frame);
		}
		Ok(supervisor)
	}

	pub async fn run(mut self) {
		let code = self.serve().await;
		self.shared.video.stop_sender();
		self.link.close().await;
		self.shared.engine.lock().take();
		self.shared.hub.emit(VoiceEvent::Disconnected {
			code,
			reconnecting: false,
		});
	}

	async fn serve(&mut self) -> Option<u16> {
		loop {
			let exit = match self.drive().await {
				Drive::Shutdown => return None,
				Drive::Exit(exit) => exit,
			};
			let code = match exit {
				Exit::Closed(code) => code,
				Exit::Lost => None,
			};
			if matches!(classify(code), Next::Stop) {
				return code;
			}
			match self.recover(code).await {
				Recovery::Recovered => {}
				Recovery::Stopped(code) => return code,
				Recovery::Shutdown => return None,
			}
		}
	}

	async fn drive(&mut self) -> Drive {
		loop {
			if let Err(exit) = self.flush_dave().await {
				return Drive::Exit(exit);
			}
			if let Err(exit) = self.flush_video().await {
				return Drive::Exit(exit);
			}
			tokio::select! {
				frame = self.link.next() => match frame {
					Ok(frame) => self.on_frame(frame),
					Err(exit) => return Drive::Exit(exit),
				},
				command = self.commands.recv() => match command {
					Some(Control::Speaking(speaking)) => {
						self.speaking = speaking;
						if let Err(exit) = self.link.send(speaking_frame(speaking, self.ssrc)).await {
							return Drive::Exit(exit);
						}
					}
					Some(Control::Video(command)) => {
						if let Err(exit) = self.on_video_control(command).await {
							return Drive::Exit(exit);
						}
					}
					None => return Drive::Shutdown,
				},
				_ = self.shutdown.changed() => return Drive::Shutdown,
			}
		}
	}

	fn on_frame(&mut self, frame: Frame) {
		match frame.body {
			Body::Json(data) => self.on_json(frame.op, data),
			Body::Binary(payload) => {
				let outgoing = self.dave.on_binary(frame.op, &payload);
				self.outbox.extend(outgoing);
			}
		}
	}

	fn on_json(&mut self, op: u8, data: Value) {
		match op {
			opcode::SPEAKING => {
				if let Ok(event) = serde_json::from_value::<Speaking>(data) {
					self.on_speaking(&event);
				}
			}
			opcode::CLIENTS_CONNECT => {
				if let Ok(event) = serde_json::from_value::<ClientsConnect>(data) {
					for user_id in event.user_ids {
						self.dave.user_joined(user_id.get());
						self.shared.hub.emit(VoiceEvent::UserJoined { user_id });
					}
				}
			}
			opcode::VIDEO => {
				if let Ok(event) = serde_json::from_value::<VideoAnnouncement>(data) {
					self.on_video_announcement(&event);
				}
			}
			opcode::MEDIA_SINK_WANTS => self.on_sink_wants(&data),
			opcode::CLIENT_DISCONNECT => {
				if let Ok(event) = serde_json::from_value::<ClientDisconnect>(data) {
					self.drop_remote_video(event.user_id);
					self.dave.user_left(event.user_id.get());
					self.shared
						.ssrc_users
						.lock()
						.retain(|_, user| *user != event.user_id);
					self.shared.hub.emit(VoiceEvent::UserLeft {
						user_id: event.user_id,
					});
				}
			}
			opcode::DAVE_PREPARE_TRANSITION
			| opcode::DAVE_EXECUTE_TRANSITION
			| opcode::DAVE_PREPARE_EPOCH => {
				let outgoing = self.dave.on_json(op, &data);
				self.outbox.extend(outgoing);
			}
			opcode::HEARTBEAT_ACK => self.on_ack(),
			_ => {}
		}
	}

	async fn flush_dave(&mut self) -> std::result::Result<(), Exit> {
		for event in self.dave.take_events() {
			self.shared.hub.emit(match event {
				DaveEvent::Epoch {
					epoch,
					version,
					members,
				} => VoiceEvent::Dave {
					epoch,
					version,
					members: members.into_iter().map(Snowflake).collect(),
				},
				DaveEvent::Failure(reason) => VoiceEvent::DaveFailure(reason),
			});
		}
		for outgoing in std::mem::take(&mut self.outbox) {
			self.link.send_message(outgoing.encode()).await?;
		}
		Ok(())
	}

	async fn flush_video(&mut self) -> std::result::Result<(), Exit> {
		if !std::mem::take(&mut self.reannounce) {
			return Ok(());
		}
		if let Some((_, quality)) = self.announced {
			self.link.send(video_frame(self.ssrcs, true, &quality)).await?;
			self.shared.video.set_ssrc(self.ssrcs.video);
		}
		Ok(())
	}

	async fn on_video_control(&mut self, command: VideoControl) -> std::result::Result<(), Exit> {
		match command {
			VideoControl::Announce { kind, quality } => {
				self.link.send(video_frame(self.ssrcs, true, &quality)).await?;
				let first = self.announced.replace((kind, quality)).is_none();
				self.shared.video.wanted.store(true, Ordering::Relaxed);
				self.shared.video.set_ssrc(self.ssrcs.video);
				if first {
					self.shared.hub.emit(VoiceEvent::VideoStarted {
						user_id: self.credentials.user_id(),
						ssrc: self.ssrcs.video,
						kind,
					});
				}
			}
			VideoControl::Active(active) => {
				if let Some((_, quality)) = self.announced {
					self.link.send(video_frame(self.ssrcs, active, &quality)).await?;
				}
			}
			VideoControl::Clear => {
				if let Some((kind, _)) = self.announced.take() {
					self.shared.video.set_ssrc(0);
					self.link.send(clear_video_frame(self.ssrcs.audio)).await?;
					self.shared.hub.emit(VoiceEvent::VideoStopped {
						user_id: self.credentials.user_id(),
						ssrc: self.ssrcs.video,
						kind,
					});
				}
			}
		}
		Ok(())
	}

	fn on_video_announcement(&mut self, event: &VideoAnnouncement) {
		let Some(user_id) = event.user_id else {
			return;
		};
		if user_id == self.credentials.user_id() {
			return;
		}
		let kind = self.shared.video.kind();
		let mut ssrcs: Vec<u32> = event
			.streams
			.iter()
			.filter(|stream| stream.active != Some(false))
			.map(|stream| stream.ssrc)
			.chain([event.video_ssrc])
			.filter(|ssrc| *ssrc != 0)
			.collect();
		ssrcs.sort_unstable();
		ssrcs.dedup();
		{
			let mut users = self.shared.ssrc_users.lock();
			if event.audio_ssrc != 0 {
				users.insert(event.audio_ssrc, user_id);
			}
			for ssrc in &ssrcs {
				users.insert(*ssrc, user_id);
			}
		}
		self.dave.user_joined(user_id.get());
		let primary = if event.video_ssrc != 0 {
			event.video_ssrc
		} else {
			ssrcs.first().copied().unwrap_or_default()
		};
		let ended: Vec<VideoStream> = {
			let mut remote = self.shared.video.remote.lock();
			let ended = remote
				.values()
				.filter(|stream| stream.user_id == user_id && !ssrcs.contains(&stream.ssrc))
				.copied()
				.collect::<Vec<_>>();
			for stream in &ended {
				remote.remove(&stream.ssrc);
			}
			ended
		};
		for stream in ended {
			self.end_remote_video(stream);
		}
		for ssrc in ssrcs {
			let stream = VideoStream { user_id, ssrc, kind };
			let inserted = self.shared.video.remote.lock().insert(ssrc, stream).is_none();
			if inserted && ssrc == primary {
				self.shared
					.hub
					.emit(VoiceEvent::VideoStarted { user_id, ssrc, kind });
			}
		}
	}

	fn drop_remote_video(&mut self, user_id: Snowflake) {
		let ended: Vec<VideoStream> = {
			let mut remote = self.shared.video.remote.lock();
			let ended = remote
				.values()
				.filter(|stream| stream.user_id == user_id)
				.copied()
				.collect::<Vec<_>>();
			for stream in &ended {
				remote.remove(&stream.ssrc);
			}
			ended
		};
		for stream in ended {
			self.end_remote_video(stream);
		}
	}

	fn end_remote_video(&self, stream: VideoStream) {
		let sink = self.shared.video.sink.lock().clone();
		if let Some(sink) = sink {
			sink.ended(stream);
		}
		self.shared.hub.emit(VoiceEvent::VideoStopped {
			user_id: stream.user_id,
			ssrc: stream.ssrc,
			kind: stream.kind,
		});
	}

	fn on_sink_wants(&mut self, data: &Value) {
		let Some(wants) = data.as_object() else {
			return;
		};
		let ssrc = self.shared.video.ssrc();
		if ssrc == 0 {
			return;
		}
		let level = wants
			.get(&ssrc.to_string())
			.or_else(|| wants.get("any"))
			.and_then(Value::as_u64);
		let Some(level) = level else {
			return;
		};
		let wanted = level > 0;
		if self.shared.video.wanted.swap(wanted, Ordering::Relaxed) != wanted {
			if wanted {
				self.shared.video.keyframe_wanted.store(true, Ordering::Relaxed);
			}
			let _ = self.control.send(Control::Video(VideoControl::Active(wanted)));
		}
	}

	fn on_speaking(&mut self, event: &Speaking) {
		let Some(user_id) = event.user_id else {
			return;
		};
		if let Some(ssrc) = event.ssrc {
			self.shared.ssrc_users.lock().insert(ssrc, user_id);
		}
		self.dave.user_joined(user_id.get());
		self.shared.hub.emit(VoiceEvent::Speaking {
			user_id,
			speaking: event.speaking != 0,
		});
	}

	fn on_ack(&self) {
		let Some(ping_ms) = self.link.ping_ms else {
			return;
		};
		let counters = &self.shared.counters;
		counters.set_ping(ping_ms);
		self.shared.hub.emit(VoiceEvent::Stats {
			ping_ms,
			packets_lost: counters.lost.load(Ordering::Relaxed),
			jitter_ms: counters.jitter_ms.load(),
		});
	}

	async fn recover(&mut self, code: Option<u16>) -> Recovery {
		self.shared.hub.emit(VoiceEvent::Disconnected {
			code,
			reconnecting: true,
		});
		let mut code = code;
		if matches!(classify(code), Next::Resume) {
			for attempt in 0..RESUME_ATTEMPTS {
				if !self.pause(backoff(attempt)).await {
					return Recovery::Shutdown;
				}
				match resume(&self.credentials, self.link.last_seq).await {
					Ok(link) => {
						self.link = link;
						return self.after_resume().await;
					}
					Err(VoiceError::Closed(closed)) => {
						code = Some(closed);
						match classify(code) {
							Next::Resume => {}
							Next::Fresh => break,
							Next::Stop => return Recovery::Stopped(code),
						}
					}
					Err(error) => debug!(%error, "voice resume attempt failed"),
				}
				if attempt + 1 == RESUME_ATTEMPTS {
					return Recovery::Stopped(code);
				}
			}
		}
		for attempt in 0..FRESH_ATTEMPTS {
			if !self.pause(backoff(attempt)).await {
				return Recovery::Shutdown;
			}
			match establish(&self.credentials).await {
				Ok(established) => return self.adopt(established),
				Err(VoiceError::Closed(closed)) => return Recovery::Stopped(Some(closed)),
				Err(error) => debug!(%error, "voice reconnect attempt failed"),
			}
		}
		Recovery::Stopped(code)
	}

	async fn after_resume(&mut self) -> Recovery {
		if self
			.link
			.send(speaking_frame(self.speaking, self.ssrc))
			.await
			.is_err()
		{
			return Recovery::Stopped(None);
		}
		self.shared.hub.emit(VoiceEvent::Connected { ssrc: self.ssrc });
		Recovery::Recovered
	}

	fn adopt(&mut self, established: Established) -> Recovery {
		self.shared.ssrc_users.lock().clear();
		self.shared.video.remote.lock().clear();
		self.ssrc = established.transport.ssrc;
		self.ssrcs = established.ssrcs;
		self.shared
			.video
			.keyframe_interval_ms
			.store(established.keyframe_interval_ms, Ordering::Relaxed);
		if self.announced.is_some() {
			self.shared.video.set_ssrc(0);
			self.reannounce = true;
		}
		self.speaking = false;
		match spawn_media(
			established.transport,
			&self.shared,
			&self.control,
			&self.transform,
			self.audio_io,
		) {
			Ok(media) => self.media = media,
			Err(error) => {
				warn!(%error, "failed to restart voice media");
				return Recovery::Stopped(None);
			}
		}
		self.link = established.link;
		self.outbox = self.dave.on_protocol_version(established.dave_version);
		while let Some(frame) = self.link.backlog.pop_front() {
			self.on_frame(frame);
		}
		self.shared.hub.emit(VoiceEvent::Connected { ssrc: self.ssrc });
		Recovery::Recovered
	}

	async fn pause(&mut self, duration: Duration) -> bool {
		tokio::select! {
			() = sleep(duration) => true,
			_ = self.shutdown.changed() => false,
		}
	}
}

fn spawn_media(
	transport: Transport,
	shared: &Arc<Shared>,
	control: &mpsc::UnboundedSender<Control>,
	transform: &SharedTransform,
	audio_io: bool,
) -> Result<MediaHandle> {
	let media = Media::new(
		transport,
		shared.clone(),
		control.clone(),
		transform.clone(),
		audio_io,
	)?;
	Ok(MediaHandle(tokio::spawn(media.run())))
}
