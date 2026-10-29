use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Callback, ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use super::RpcShared;
use super::link::{Close, Link, Transport};

const FIRST_PORT: u16 = 6463;
const PORT_COUNT: u16 = 10;
const ACCEPT_RETRY: Duration = Duration::from_millis(100);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

struct Opening {
	version: Option<i64>,
	client_id: Option<String>,
	encoding: Option<String>,
	origin: Option<String>,
}

struct Capture<'a>(&'a mut Option<Opening>);

impl Callback for Capture<'_> {
	fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
		*self.0 = Some(opening(request));
		Ok(response)
	}
}

pub(crate) async fn bind() -> Option<(u16, TcpListener)> {
	for port in FIRST_PORT..FIRST_PORT + PORT_COUNT {
		if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
			return Some((port, listener));
		}
	}
	tracing::warn!("all rpc ports are taken");
	None
}

pub(crate) async fn accept_loop(
	shared: Arc<RpcShared>,
	listener: TcpListener,
	mut enabled: watch::Receiver<bool>,
) {
	loop {
		tokio::select! {
			accepted = listener.accept() => match accepted {
				Ok((stream, _)) => {
					if shared.admit() {
						tokio::spawn(run(shared.clone(), stream, enabled.clone()));
					}
				}
				Err(error) => {
					tracing::debug!(%error, "rpc socket accept failed");
					sleep(ACCEPT_RETRY).await;
				}
			},
			changed = enabled.changed() => {
				if changed.is_err() || !*enabled.borrow() {
					return;
				}
			}
		}
	}
}

fn opening(request: &Request) -> Opening {
	let query = request.uri().query().unwrap_or_default();
	let mut opening = Opening {
		version: None,
		client_id: None,
		encoding: None,
		origin: request
			.headers()
			.get("origin")
			.and_then(|value| value.to_str().ok())
			.map(str::to_owned),
	};
	for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
		match key.as_ref() {
			"v" => opening.version = value.parse().ok(),
			"client_id" => opening.client_id = Some(value.into_owned()),
			"encoding" => opening.encoding = Some(value.into_owned()),
			_ => {}
		}
	}
	opening
}

async fn run(shared: Arc<RpcShared>, stream: TcpStream, mut enabled: watch::Receiver<bool>) {
	let mut captured = None;
	let accepted = timeout(
		HANDSHAKE_TIMEOUT,
		accept_hdr_async(stream, Capture(&mut captured)),
	)
	.await;
	let (Ok(Ok(mut socket)), Some(opening)) = (accepted, captured) else {
		shared.release();
		return;
	};
	let mut link = Link::new(shared.clone(), Transport::WebSocket);
	tokio::select! {
		() = session(&mut link, &mut socket, opening) => {}
		_ = enabled.changed() => {
			let _ = socket.close(None).await;
		}
	}
	shared.release();
}

async fn reject(socket: &mut WebSocketStream<TcpStream>, close: Close) {
	let _ = socket
		.close(Some(CloseFrame {
			code: CloseCode::Library(close.code),
			reason: close.message.into(),
		}))
		.await;
}

async fn session(link: &mut Link, socket: &mut WebSocketStream<TcpStream>, opening: Opening) {
	if opening
		.encoding
		.as_deref()
		.is_some_and(|encoding| encoding != "json")
	{
		return reject(socket, Close::INVALID_ENCODING).await;
	}
	let ready = match link
		.handshake(
			opening.version,
			opening.client_id.as_deref(),
			opening.origin.as_deref(),
		)
		.await
	{
		Ok(ready) => ready,
		Err(reason) => return reject(socket, reason).await,
	};
	if socket.send(Message::text(ready.to_string())).await.is_err() {
		return;
	}
	while let Some(Ok(message)) = socket.next().await {
		let text = match message {
			Message::Text(text) => text,
			Message::Close(_) => return,
			_ => continue,
		};
		let Ok(command) = serde_json::from_str::<Value>(&text) else {
			return reject(socket, Close::INVALID_ENCODING).await;
		};
		let response = link.command(&command).await;
		if socket.send(Message::text(response.to_string())).await.is_err() {
			return;
		}
	}
}
