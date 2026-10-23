use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{AeadInOut, KeyInit, Nonce};
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use rand::Rng;
use serde_json::{Value, json};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::json::parse_id;

pub const VOICE_TOKEN: &str = "mock-voice-token";

const ENCRYPTION_MODE: &str = "aead_aes256_gcm_rtpsize";
const HEARTBEAT_INTERVAL_MS: u64 = 41_250;
const FIRST_SSRC: u32 = 4_000;
const ECHO_SSRC_OFFSET: u32 = 1;
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
const COUNTER_LEN: usize = 4;
const RTP_HEADER_LEN: usize = 12;
const SSRC_RANGE: std::ops::Range<usize> = 8..12;
const DISCOVERY_PACKET_LEN: usize = 74;
const DISCOVERY_LENGTH_FIELD: u16 = 70;
const DISCOVERY_REQUEST: u16 = 1;
const DISCOVERY_RESPONSE: u16 = 2;
const DISCOVERY_ADDRESS_START: usize = 8;
const DISCOVERY_PORT_START: usize = 72;
const UDP_BUFFER_BYTES: usize = 2048;
const SPEAKING_CAPACITY: usize = 64;

struct Voice {
	key: [u8; KEY_LEN],
	udp_port: u16,
	next_ssrc: AtomicU32,
	speaking: broadcast::Sender<String>,
	shutdown: watch::Receiver<bool>,
}

struct Peer {
	user_id: Option<u64>,
	ssrc: u32,
}

pub struct VoiceEndpoint {
	pub url: String,
	pub tasks: Vec<JoinHandle<()>>,
}

pub async fn spawn(shutdown: watch::Receiver<bool>) -> io::Result<VoiceEndpoint> {
	let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
	let socket = UdpSocket::bind(("127.0.0.1", 0)).await?;
	let url = format!("ws://{}", listener.local_addr()?);
	let voice = Arc::new(Voice {
		key: rand::rng().random(),
		udp_port: socket.local_addr()?.port(),
		next_ssrc: AtomicU32::new(FIRST_SSRC),
		speaking: broadcast::channel(SPEAKING_CAPACITY).0,
		shutdown: shutdown.clone(),
	});
	let router = Router::new().route("/", get(upgrade)).with_state(voice.clone());
	let mut graceful = shutdown;
	let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
		let _ = graceful.wait_for(|stopped| *stopped).await;
	});
	let signaling = tokio::spawn(async move {
		if let Err(error) = serve.await {
			tracing::error!(%error, "mock voice server stopped");
		}
	});
	let media = tokio::spawn(echo(socket, voice));
	Ok(VoiceEndpoint {
		url,
		tasks: vec![signaling, media],
	})
}

async fn upgrade(State(voice): State<Arc<Voice>>, socket: WebSocketUpgrade) -> Response {
	socket.on_upgrade(move |socket| signal(voice, socket))
}

async fn send(socket: &mut WebSocket, frame: &Value) -> bool {
	socket.send(Message::Text(frame.to_string().into())).await.is_ok()
}

async fn signal(voice: Arc<Voice>, mut socket: WebSocket) {
	let hello = json!({ "op": 8, "d": { "heartbeat_interval": HEARTBEAT_INTERVAL_MS } });
	if !send(&mut socket, &hello).await {
		return;
	}
	let mut speaking = voice.speaking.subscribe();
	let mut shutdown = voice.shutdown.clone();
	let mut peer = Peer {
		user_id: None,
		ssrc: 0,
	};
	loop {
		tokio::select! {
			incoming = socket.recv() => match incoming {
				Some(Ok(Message::Text(text))) => {
					let Ok(frame) = serde_json::from_str::<Value>(text.as_str()) else {
						continue;
					};
					if !on_frame(&voice, &mut socket, &mut peer, &frame).await {
						break;
					}
				}
				Some(Ok(Message::Close(_)) | Err(_)) | None => break,
				Some(Ok(_)) => {}
			},
			broadcast = speaking.recv() => match broadcast {
				Ok(frame) => {
					if socket.send(Message::Text(frame.into())).await.is_err() {
						break;
					}
				}
				Err(broadcast::error::RecvError::Lagged(_)) => {}
				Err(broadcast::error::RecvError::Closed) => break,
			},
			_ = shutdown.changed() => break,
		}
	}
}

async fn on_frame(voice: &Voice, socket: &mut WebSocket, peer: &mut Peer, frame: &Value) -> bool {
	let data = &frame["d"];
	match frame["op"].as_u64() {
		Some(0) => {
			peer.user_id = parse_id(&data["user_id"]);
			peer.ssrc = voice.next_ssrc.fetch_add(1, Ordering::Relaxed);
			let ready = json!({
				"op": 2,
				"d": {
					"ssrc": peer.ssrc,
					"ip": "127.0.0.1",
					"port": voice.udp_port,
					"modes": [ENCRYPTION_MODE],
				},
			});
			send(socket, &ready).await
		}
		Some(1) => {
			let description = json!({
				"op": 4,
				"d": {
					"mode": ENCRYPTION_MODE,
					"secret_key": voice.key.to_vec(),
					"dave_protocol_version": 0,
				},
			});
			send(socket, &description).await
		}
		Some(3) => send(socket, &json!({ "op": 6, "d": { "t": data["t"] } })).await,
		Some(5) => {
			let speaking = json!({
				"op": 5,
				"d": {
					"speaking": data["speaking"],
					"ssrc": peer.ssrc,
					"user_id": peer.user_id.map(|id| id.to_string()),
				},
			});
			let _ = voice.speaking.send(speaking.to_string());
			true
		}
		Some(7) => send(socket, &json!({ "op": 9, "d": null })).await,
		_ => true,
	}
}

async fn echo(socket: UdpSocket, voice: Arc<Voice>) {
	let Ok(cipher) = Aes256Gcm::new_from_slice(&voice.key) else {
		return;
	};
	let mut shutdown = voice.shutdown.clone();
	let mut buffer = [0u8; UDP_BUFFER_BYTES];
	loop {
		tokio::select! {
			received = socket.recv_from(&mut buffer) => {
				let Ok((length, from)) = received else {
					continue;
				};
				let packet = &buffer[..length];
				let reply = if is_discovery(packet) {
					Some(discovery_reply(packet, from))
				} else {
					echoed(&cipher, packet)
				};
				if let Some(reply) = reply {
					let _ = socket.send_to(&reply, from).await;
				}
			}
			_ = shutdown.changed() => break,
		}
	}
}

fn is_discovery(packet: &[u8]) -> bool {
	packet.len() == DISCOVERY_PACKET_LEN && packet[..2] == DISCOVERY_REQUEST.to_be_bytes()
}

fn discovery_reply(request: &[u8], from: std::net::SocketAddr) -> Vec<u8> {
	let mut reply = vec![0u8; DISCOVERY_PACKET_LEN];
	reply[..2].copy_from_slice(&DISCOVERY_RESPONSE.to_be_bytes());
	reply[2..4].copy_from_slice(&DISCOVERY_LENGTH_FIELD.to_be_bytes());
	reply[4..8].copy_from_slice(&request[4..8]);
	let address = from.ip().to_string();
	reply[DISCOVERY_ADDRESS_START..DISCOVERY_ADDRESS_START + address.len()]
		.copy_from_slice(address.as_bytes());
	reply[DISCOVERY_PORT_START..].copy_from_slice(&from.port().to_be_bytes());
	reply
}

fn echoed(cipher: &Aes256Gcm, packet: &[u8]) -> Option<Vec<u8>> {
	if packet.len() < RTP_HEADER_LEN + TAG_LEN + COUNTER_LEN {
		return None;
	}
	let (sealed, counter) = packet.split_at(packet.len() - COUNTER_LEN);
	let (header, body) = sealed.split_at(RTP_HEADER_LEN);
	let mut nonce = Nonce::<Aes256Gcm>::default();
	nonce[..COUNTER_LEN].copy_from_slice(counter);
	let mut payload = body.to_vec();
	cipher.decrypt_in_place(&nonce, header, &mut payload).ok()?;
	let mut echo_header = header.to_vec();
	let ssrc = u32::from_be_bytes(header[SSRC_RANGE].try_into().ok()?);
	echo_header[SSRC_RANGE].copy_from_slice(&ssrc.wrapping_add(ECHO_SSRC_OFFSET).to_be_bytes());
	cipher.encrypt_in_place(&nonce, &echo_header, &mut payload).ok()?;
	echo_header.extend_from_slice(&payload);
	echo_header.extend_from_slice(counter);
	Some(echo_header)
}
