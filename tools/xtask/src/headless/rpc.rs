use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::Stream as _;
use mockcord::{RPC_APP_ID, RPC_APP_NAME, RPC_ASSET_ID, RPC_ASSET_NAME};
use protocol::Snowflake;
use serde_json::{Value, json};
use session::{RpcEndpoints, ipc_socket_name};
use state::State;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Instant, sleep, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use super::{Harness, STEP_TIMEOUT};

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;
const CLOSE_INVALID_CLIENT_ID: u16 = 4000;
const CLOSE_INVALID_ORIGIN: u16 = 4001;
const ERROR_INVALID_PAYLOAD: i64 = 4000;
const PROBE_DETAILS: &str = "headless rich presence";
const PROBE_STATE: &str = "probing";
const BUTTON_LABEL: &str = "Open";
const BUTTON_URL: &str = "https://example.com/rpc";
const BAD_CLIENT_ID: &str = "1";
const BAD_ORIGIN: &str = "https://evil.example";
const ENDPOINT_POLL: Duration = Duration::from_millis(50);

async fn write_frame(stream: &mut Stream, opcode: u32, payload: &Value) -> Result<()> {
	let body = payload.to_string();
	let mut frame = Vec::with_capacity(8 + body.len());
	frame.extend_from_slice(&opcode.to_le_bytes());
	frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
	frame.extend_from_slice(body.as_bytes());
	stream.write_all(&frame).await.context("rpc pipe write")
}

async fn read_frame(stream: &mut Stream) -> Result<(u32, Value)> {
	let read = async {
		let mut header = [0u8; 8];
		stream.read_exact(&mut header).await?;
		let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
		let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
		let mut payload = vec![0u8; length];
		stream.read_exact(&mut payload).await?;
		anyhow::Ok((opcode, serde_json::from_slice::<Value>(&payload)?))
	};
	timeout(STEP_TIMEOUT, read)
		.await
		.context("timed out waiting for an rpc pipe frame")?
}

fn probe_activity(start_secs: i64) -> Value {
	json!({
		"details": PROBE_DETAILS,
		"state": PROBE_STATE,
		"timestamps": { "start": start_secs },
		"assets": { "large_image": RPC_ASSET_NAME, "large_text": "Headless" },
		"buttons": [{ "label": BUTTON_LABEL, "url": BUTTON_URL }],
	})
}

fn check_ready(ready: &Value, me: Snowflake) -> Result<()> {
	ensure!(ready["evt"] == "READY", "rpc did not dispatch READY: {ready}");
	ensure!(ready["cmd"] == "DISPATCH", "READY is not a DISPATCH: {ready}");
	ensure!(ready["data"]["v"] == 1, "READY has the wrong rpc version");
	ensure!(
		ready["data"]["config"]["cdn_host"] == "cdn.discordapp.com"
			&& ready["data"]["config"]["api_endpoint"] == "//discord.com/api"
			&& ready["data"]["config"]["environment"] == "production",
		"READY config does not match the Discord client: {ready}"
	);
	ensure!(
		ready["data"]["user"]["id"] == me.to_string(),
		"READY user is not the signed in user: {ready}"
	);
	Ok(())
}

fn check_activity(response: &Value, nonce: &str, start_secs: i64) -> Result<()> {
	ensure!(
		response["cmd"] == "SET_ACTIVITY",
		"unexpected rpc response: {response}"
	);
	ensure!(response["nonce"] == nonce, "rpc response nonce was not echoed");
	ensure!(
		response["evt"].is_null(),
		"SET_ACTIVITY returned an event: {response}"
	);
	let data = &response["data"];
	ensure!(
		data["application_id"]
			.as_str()
			.and_then(|id| id.parse::<u64>().ok())
			== Some(RPC_APP_ID),
		"activity application id is wrong"
	);
	ensure!(
		data["name"] == RPC_APP_NAME,
		"activity name was not taken from the application"
	);
	ensure!(data["type"] == 0, "activity type is not playing");
	ensure!(
		data["details"] == PROBE_DETAILS && data["state"] == PROBE_STATE,
		"activity text was altered"
	);
	ensure!(
		data["timestamps"]["start"] == start_secs * 1000,
		"seconds timestamp was not normalized to milliseconds"
	);
	ensure!(
		data["assets"]["large_image"]
			.as_str()
			.and_then(|id| id.parse::<u64>().ok())
			== Some(RPC_ASSET_ID),
		"asset name was not resolved to the asset id: {data}"
	);
	ensure!(
		data["buttons"] == json!([BUTTON_LABEL]),
		"button labels were not split out"
	);
	ensure!(
		data["metadata"]["button_urls"] == json!([BUTTON_URL]),
		"button urls were not moved to metadata"
	);
	Ok(())
}

fn has_probe(state: &State, me: Snowflake) -> bool {
	state.presence(me).is_some_and(|presence| {
		presence
			.activities
			.iter()
			.any(|activity| activity.application_id == Some(Snowflake(RPC_APP_ID)))
	})
}

fn now_secs() -> i64 {
	chrono::Utc::now().timestamp()
}

async fn expect_close(url: &str, origin: Option<&str>, code: u16) -> Result<()> {
	let mut request = url.into_client_request()?;
	if let Some(origin) = origin {
		request.headers_mut().insert("Origin", origin.parse()?);
	}
	let (mut socket, _) = connect_async(request).await.context("rpc websocket connect")?;
	let message = timeout(STEP_TIMEOUT, socket.next())
		.await
		.context("timed out waiting for the rpc websocket close")?;
	match message {
		Some(Ok(Message::Close(Some(frame)))) => {
			ensure!(
				frame.code == CloseCode::Library(code),
				"rpc websocket closed with {:?} instead of {code}",
				frame.code
			);
			Ok(())
		}
		other => bail!("rpc websocket did not close with {code}: {other:?}"),
	}
}

impl Harness {
	async fn rpc_endpoints(&self) -> Result<RpcEndpoints> {
		let deadline = Instant::now() + STEP_TIMEOUT;
		loop {
			let endpoints = self.session.rpc_endpoints();
			if endpoints.ipc.is_some() && endpoints.port.is_some() {
				return Ok(endpoints);
			}
			ensure!(
				Instant::now() < deadline,
				"timed out waiting for the rpc server to bind"
			);
			sleep(ENDPOINT_POLL).await;
		}
	}

	async fn rpc_over_ipc(&self, index: u8, me: Snowflake) -> Result<()> {
		let mut stream = Stream::connect(ipc_socket_name(index)?)
			.await
			.context("rpc pipe connect")?;
		write_frame(
			&mut stream,
			OP_HANDSHAKE,
			&json!({ "v": 1, "client_id": RPC_APP_ID.to_string() }),
		)
		.await?;
		let (opcode, ready) = read_frame(&mut stream).await?;
		ensure!(opcode == OP_FRAME, "handshake was not answered with a frame");
		check_ready(&ready, me)?;

		write_frame(&mut stream, OP_PING, &json!({ "nonce": "ping" })).await?;
		let (opcode, pong) = read_frame(&mut stream).await?;
		ensure!(
			opcode == OP_PONG && pong["nonce"] == "ping",
			"ping was not echoed as pong"
		);

		let start_secs = now_secs();
		write_frame(
			&mut stream,
			OP_FRAME,
			&json!({
				"cmd": "SET_ACTIVITY",
				"args": { "pid": std::process::id(), "activity": probe_activity(start_secs) },
				"nonce": "set-1",
			}),
		)
		.await?;
		let (opcode, response) = read_frame(&mut stream).await?;
		ensure!(opcode == OP_FRAME, "SET_ACTIVITY was not answered with a frame");
		check_activity(&response, "set-1", start_secs)?;
		self.until("the rich presence to reach the gateway", |state| {
			has_probe(state, me)
		})
		.await?;

		write_frame(
			&mut stream,
			OP_FRAME,
			&json!({
				"cmd": "SET_ACTIVITY",
				"args": { "pid": std::process::id(), "activity": { "state": "x" } },
				"nonce": "set-2",
			}),
		)
		.await?;
		let (_, rejected) = read_frame(&mut stream).await?;
		ensure!(
			rejected["evt"] == "ERROR" && rejected["data"]["code"] == ERROR_INVALID_PAYLOAD,
			"a one character state was accepted: {rejected}"
		);
		ensure!(
			rejected["data"]["message"].as_str().is_some_and(
				|message| message.contains("\"state\" length must be at least 2 characters long")
			),
			"validation message does not match Discord: {rejected}"
		);

		drop(stream);
		self.until("the rich presence to clear after the pipe closed", |state| {
			!has_probe(state, me)
		})
		.await
	}

	async fn rpc_over_websocket(&self, port: u16, me: Snowflake) -> Result<()> {
		let base = format!("ws://127.0.0.1:{port}/?v=1&encoding=json");
		expect_close(
			&format!("{base}&client_id={BAD_CLIENT_ID}"),
			None,
			CLOSE_INVALID_CLIENT_ID,
		)
		.await?;
		expect_close(
			&format!("{base}&client_id={RPC_APP_ID}"),
			Some(BAD_ORIGIN),
			CLOSE_INVALID_ORIGIN,
		)
		.await?;

		let (mut socket, _) = connect_async(format!("{base}&client_id={RPC_APP_ID}"))
			.await
			.context("rpc websocket connect")?;
		let ready = match timeout(STEP_TIMEOUT, socket.next()).await? {
			Some(Ok(Message::Text(text))) => serde_json::from_str::<Value>(&text)?,
			other => bail!("rpc websocket did not send READY: {other:?}"),
		};
		check_ready(&ready, me)?;
		let start_secs = now_secs();
		socket
			.send(Message::text(
				json!({
					"cmd": "SET_ACTIVITY",
					"args": { "activity": probe_activity(start_secs) },
					"nonce": "ws-1",
				})
				.to_string(),
			))
			.await?;
		let response = match timeout(STEP_TIMEOUT, socket.next()).await? {
			Some(Ok(Message::Text(text))) => serde_json::from_str::<Value>(&text)?,
			other => bail!("rpc websocket did not answer SET_ACTIVITY: {other:?}"),
		};
		check_activity(&response, "ws-1", start_secs)?;
		self.until("the websocket rich presence to reach the gateway", |state| {
			has_probe(state, me)
		})
		.await?;
		socket.close(None).await?;
		drop(socket);
		self.until("the rich presence to clear after the websocket closed", |state| {
			!has_probe(state, me)
		})
		.await
	}

	pub(super) async fn rpc_features(&self) -> Result<()> {
		let endpoints = self.rpc_endpoints().await?;
		let me = self.state.read().current_user_id().context("no signed in user")?;
		let (Some(index), Some(port)) = (endpoints.ipc, endpoints.port) else {
			bail!("rpc server endpoints are missing");
		};
		self.rpc_over_ipc(index, me).await?;
		self.rpc_over_websocket(port, me).await
	}
}
