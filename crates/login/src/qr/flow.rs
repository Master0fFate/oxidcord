use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use protocol::Token;
use rest::{Request, RestClient, RestConfig};
use serde::Serialize;
use tokio::sync::mpsc::Sender;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep_until, timeout};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as SocketError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};
use url::{Host, Url};

use super::bitmap::QrBitmap;
use super::crypto::{SessionKey, nonce_proof};
use super::messages::{Incoming, Outgoing, TicketExchange, TokenGrant};
use super::{QrConfig, QrEvent};
use crate::error::{LoginError, Result};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
const MIN_HEARTBEAT: Duration = Duration::from_secs(1);
const CODE_URL_PREFIX: &str = "https://discord.com/ra/";
const LOGIN_PATH: &str = "/users/@me/remote-auth/login";
const CAPTCHA_HINT: &str = "Discord refused the ticket exchange, it may need a captcha. Use the web login";

struct Flow<'a> {
	events: &'a Sender<QrEvent>,
	key: SessionKey,
	rest: RestClient,
}

pub(super) async fn drive(config: QrConfig, events: Sender<QrEvent>) {
	let event = match run(&config, &events).await {
		Ok(token) => QrEvent::Token(token),
		Err(LoginError::Expired) => QrEvent::Expired,
		Err(LoginError::Cancelled) => QrEvent::Cancelled,
		Err(error) => QrEvent::Error(error.to_string()),
	};
	let _ = events.send(event).await;
}

fn loopback(url: &Url) -> bool {
	match url.host() {
		Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
		Some(Host::Ipv4(address)) => address.is_loopback(),
		Some(Host::Ipv6(address)) => address.is_loopback(),
		None => false,
	}
}

fn check_gateway_url(url: &Url) -> Result<()> {
	match url.scheme() {
		"wss" => Ok(()),
		"ws" if loopback(url) => Ok(()),
		_ => Err(LoginError::Protocol("insecure gateway url".to_owned())),
	}
}

fn socket_failure(error: &SocketError) -> String {
	match error {
		SocketError::Http(response) => format!("gateway refused the connection with {}", response.status()),
		SocketError::Capacity(_) => "gateway message too large".to_owned(),
		_ => "connection failed".to_owned(),
	}
}

fn network_error(error: &SocketError) -> LoginError {
	LoginError::Network(socket_failure(error))
}

fn close_error(code: u16) -> LoginError {
	match code {
		4003 => LoginError::Expired,
		1000 => LoginError::Cancelled,
		other => LoginError::Protocol(format!("gateway closed with code {other}")),
	}
}

async fn connect(config: &QrConfig) -> Result<Socket> {
	check_gateway_url(&config.gateway_url)?;
	let mut request = config
		.gateway_url
		.as_str()
		.into_client_request()
		.map_err(|_| LoginError::Protocol("invalid gateway url".to_owned()))?;
	let origin = HeaderValue::from_str(&config.origin)
		.map_err(|_| LoginError::Protocol("invalid origin".to_owned()))?;
	request.headers_mut().insert(ORIGIN, origin);
	let socket_config = WebSocketConfig::default()
		.max_frame_size(Some(MAX_MESSAGE_BYTES))
		.max_message_size(Some(MAX_MESSAGE_BYTES));
	let attempt = timeout(
		CONNECT_TIMEOUT,
		connect_async_with_config(request, Some(socket_config), true),
	)
	.await
	.map_err(|_| LoginError::Network("connect timed out".to_owned()))?;
	attempt
		.map(|(socket, _)| socket)
		.map_err(|error| network_error(&error))
}

async fn next_incoming(socket: &mut Socket) -> Result<Incoming> {
	loop {
		match socket.next().await {
			Some(Ok(Message::Text(text))) => {
				return serde_json::from_str(&text)
					.map_err(|_| LoginError::Protocol("undecodable gateway message".to_owned()));
			}
			Some(Ok(Message::Close(frame))) => {
				return Err(frame.map_or(LoginError::Cancelled, |frame| close_error(u16::from(frame.code))));
			}
			Some(Ok(_)) => {}
			Some(Err(error)) => return Err(network_error(&error)),
			None => return Err(LoginError::Network("gateway closed the connection".to_owned())),
		}
	}
}

async fn send<T: Serialize>(socket: &mut Socket, message: &T) -> Result<()> {
	let text =
		serde_json::to_string(message).map_err(|_| LoginError::Protocol("unencodable message".to_owned()))?;
	socket
		.send(Message::text(text))
		.await
		.map_err(|error| network_error(&error))
}

async fn expect_hello(socket: &mut Socket) -> Result<(Duration, Duration)> {
	match timeout(HELLO_TIMEOUT, next_incoming(socket)).await {
		Ok(Ok(Incoming::Hello {
			heartbeat_interval,
			timeout_ms,
		})) => Ok((
			Duration::from_millis(heartbeat_interval).max(MIN_HEARTBEAT),
			Duration::from_millis(timeout_ms),
		)),
		Ok(Ok(_)) => Err(LoginError::Protocol("expected hello".to_owned())),
		Ok(Err(error)) => Err(error),
		Err(_) => Err(LoginError::Network("gateway did not say hello".to_owned())),
	}
}

async fn run(config: &QrConfig, events: &Sender<QrEvent>) -> Result<Token> {
	let mut socket = connect(config).await?;
	let (heartbeat_interval, session_length) = expect_hello(&mut socket).await?;
	let key = tokio::task::spawn_blocking(SessionKey::generate)
		.await
		.map_err(|_| LoginError::Protocol("key generation aborted".to_owned()))??;
	send(
		&mut socket,
		&Outgoing::Init {
			encoded_public_key: &key.encoded_public_key(),
		},
	)
	.await?;
	let flow = Flow {
		events,
		key,
		rest: RestClient::new(RestConfig {
			base_url: config.rest_base.clone(),
			..RestConfig::default()
		}),
	};
	flow.listen(&mut socket, heartbeat_interval, Instant::now() + session_length)
		.await
}

impl Flow<'_> {
	async fn listen(&self, socket: &mut Socket, period: Duration, deadline: Instant) -> Result<Token> {
		let first_beat = Instant::now() + period.mul_f64(rand::random::<f64>());
		let mut heartbeat = interval_at(first_beat, period);
		heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
		let mut acknowledged = true;
		loop {
			tokio::select! {
				_ = sleep_until(deadline) => return Err(LoginError::Expired),
				_ = heartbeat.tick() => {
					if !acknowledged {
						return Err(LoginError::Network("gateway stopped answering heartbeats".to_owned()));
					}
					acknowledged = false;
					send(socket, &Outgoing::Heartbeat).await?;
				}
				incoming = next_incoming(socket) => match incoming? {
					Incoming::HeartbeatAck => acknowledged = true,
					Incoming::NonceProof { encrypted_nonce } => self.answer_nonce(socket, &encrypted_nonce).await?,
					Incoming::PendingRemoteInit { fingerprint } => self.show_code(&fingerprint).await?,
					Incoming::PendingTicket { encrypted_user_payload } => {
						self.announce_scan(&encrypted_user_payload).await?;
					}
					Incoming::PendingLogin { ticket } => return self.exchange(&ticket).await,
					Incoming::Cancel => return Err(LoginError::Cancelled),
					Incoming::Hello { .. } | Incoming::Unknown => {}
				},
			}
		}
	}

	async fn emit(&self, event: QrEvent) -> Result<()> {
		self.events.send(event).await.map_err(|_| LoginError::Cancelled)
	}

	async fn answer_nonce(&self, socket: &mut Socket, encrypted_nonce: &str) -> Result<()> {
		let nonce = self.key.decrypt(encrypted_nonce)?;
		send(
			socket,
			&Outgoing::NonceProof {
				nonce: &nonce_proof(&nonce),
			},
		)
		.await
	}

	async fn show_code(&self, fingerprint: &str) -> Result<()> {
		if fingerprint != self.key.fingerprint() {
			return Err(LoginError::Protocol("fingerprint mismatch".to_owned()));
		}
		let url = format!("{CODE_URL_PREFIX}{fingerprint}");
		let bitmap = QrBitmap::render(&url)?;
		self.emit(QrEvent::Code { url, bitmap }).await
	}

	async fn announce_scan(&self, encrypted_payload: &str) -> Result<()> {
		let payload = self.key.decrypt(encrypted_payload)?;
		let text = std::str::from_utf8(&payload)
			.map_err(|_| LoginError::Protocol("user payload is not utf-8".to_owned()))?;
		let mut fields = text.splitn(4, ':');
		let (Some(id), Some(_discriminator), Some(avatar), Some(username)) =
			(fields.next(), fields.next(), fields.next(), fields.next())
		else {
			return Err(LoginError::Protocol("malformed user payload".to_owned()));
		};
		self.emit(QrEvent::Scanned {
			username: username.to_owned(),
			avatar: (avatar != "0" && !avatar.is_empty()).then(|| avatar.to_owned()),
			id: id.to_owned(),
		})
		.await
	}

	async fn exchange(&self, ticket: &str) -> Result<Token> {
		let request = Request::post(LOGIN_PATH).json(&TicketExchange { ticket });
		let grant: TokenGrant = self.rest.send(request).await.map_err(exchange_error)?;
		self.key.decrypt_token(&grant.encrypted_token)
	}
}

fn exchange_error(error: rest::RestError) -> LoginError {
	match error {
		rest::RestError::Http(status, _) if status.as_u16() == 400 => {
			LoginError::Protocol(CAPTCHA_HINT.to_owned())
		}
		other => LoginError::from(other),
	}
}
