use std::sync::Arc;

use protocol::Snowflake;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::activity;
use super::{RpcEvent, RpcShared};
use crate::apps::RpcApp;
use crate::engine::{ActivityRequest, Input};

const RPC_VERSION: i64 = 1;
const CDN_HOST: &str = "cdn.discordapp.com";
const API_ENDPOINT: &str = "//discord.com/api";
const ENVIRONMENT: &str = "production";
const ERROR_INVALID_PAYLOAD: i64 = 4000;
const ERROR_INVALID_COMMAND: i64 = 4002;
const ERROR_INVALID_EVENT: i64 = 4004;
const ERROR_SERVICE_UNAVAILABLE: i64 = 1001;

const KNOWN_COMMANDS: [&str; 19] = [
	"AUTHORIZE",
	"AUTHENTICATE",
	"GET_GUILD",
	"GET_GUILDS",
	"GET_CHANNEL",
	"GET_CHANNELS",
	"GET_RELATIONSHIPS",
	"GET_USER",
	"SET_USER_VOICE_SETTINGS",
	"SELECT_VOICE_CHANNEL",
	"GET_SELECTED_VOICE_CHANNEL",
	"SELECT_TEXT_CHANNEL",
	"GET_VOICE_SETTINGS",
	"SET_VOICE_SETTINGS",
	"ACTIVITY_INVITE_USER",
	"ACCEPT_ACTIVITY_INVITE",
	"INVITE_BROWSER",
	"DEEP_LINK",
	"SET_CERTIFIED_DEVICES",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
	Ipc,
	WebSocket,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Close {
	pub code: u16,
	pub message: &'static str,
}

impl Close {
	pub const INVALID_CLIENT_ID: Self = Self {
		code: 4000,
		message: "Invalid Client ID",
	};
	pub const INVALID_ORIGIN: Self = Self {
		code: 4001,
		message: "Invalid origin",
	};
	pub const INVALID_VERSION: Self = Self {
		code: 4004,
		message: "Invalid RPC version",
	};
	pub const INVALID_ENCODING: Self = Self {
		code: 4005,
		message: "Invalid encoding",
	};

	pub fn payload(&self) -> Value {
		json!({ "code": self.code, "message": self.message })
	}
}

pub(crate) struct Link {
	shared: Arc<RpcShared>,
	id: u64,
	transport: Transport,
	app: Option<Arc<RpcApp>>,
	has_activity: bool,
	party_id: Option<String>,
	events: mpsc::UnboundedReceiver<Value>,
	sender: Option<mpsc::UnboundedSender<Value>>,
}

fn error(cmd: &Value, nonce: &Value, code: i64, message: impl Into<String>) -> Value {
	json!({
		"cmd": cmd,
		"data": { "code": code, "message": message.into() },
		"evt": "ERROR",
		"nonce": nonce,
	})
}

fn requesting_user(args: &Value) -> Option<Snowflake> {
	args.get("user_id")
		.and_then(Value::as_str)
		.and_then(|id| id.parse().ok())
		.map(Snowflake)
}

fn invalid_user_id(cmd: &Value, nonce: &Value) -> Value {
	error(
		cmd,
		nonce,
		ERROR_INVALID_PAYLOAD,
		r#"child "user_id" fails because ["user_id" must be a string]"#,
	)
}

fn reply(cmd: &Value, nonce: &Value, data: Value) -> Value {
	json!({ "cmd": cmd, "data": data, "evt": Value::Null, "nonce": nonce })
}

impl Link {
	pub fn new(shared: Arc<RpcShared>, transport: Transport) -> Self {
		let id = shared.next_link();
		let (sender, events) = mpsc::unbounded_channel();
		Self {
			shared,
			id,
			transport,
			app: None,
			has_activity: false,
			party_id: None,
			events,
			sender: Some(sender),
		}
	}

	pub async fn next_event(&mut self) -> Option<Value> {
		self.events.recv().await
	}

	pub async fn handshake(
		&mut self,
		version: Option<i64>,
		client_id: Option<&str>,
		origin: Option<&str>,
	) -> Result<Value, Close> {
		if version != Some(RPC_VERSION) {
			return Err(Close::INVALID_VERSION);
		}
		let client_id: u64 = client_id
			.and_then(|id| id.parse().ok())
			.ok_or(Close::INVALID_CLIENT_ID)?;
		let app = self
			.shared
			.apps
			.rpc_app(client_id)
			.await
			.ok_or(Close::INVALID_CLIENT_ID)?;
		if let Some(origin) = origin
			&& !app.allows_origin(origin)
		{
			return Err(Close::INVALID_ORIGIN);
		}
		tracing::debug!(client_id, app = %app.name, transport = ?self.transport, "rpc client connected");
		if let Some(sender) = self.sender.take() {
			self.shared.register(self.id, app.id.get(), sender);
		}
		self.app = Some(app);
		Ok(json!({
			"cmd": "DISPATCH",
			"data": {
				"v": RPC_VERSION,
				"config": {
					"cdn_host": CDN_HOST,
					"api_endpoint": API_ENDPOINT,
					"environment": ENVIRONMENT,
				},
				"user": self.shared.user_json(),
			},
			"evt": "READY",
			"nonce": Value::Null,
		}))
	}

	pub async fn command(&mut self, payload: &Value) -> Value {
		let cmd = payload.get("cmd").cloned().unwrap_or(Value::Null);
		let nonce = payload.get("nonce").cloned().unwrap_or(Value::Null);
		let args = payload.get("args").cloned().unwrap_or(Value::Null);
		let Some(name) = cmd.as_str() else {
			return error(
				&cmd,
				&nonce,
				ERROR_INVALID_PAYLOAD,
				r#"child "cmd" fails because ["cmd" must be a string]"#,
			);
		};
		match name {
			"SET_ACTIVITY" => self.set_activity(&cmd, &nonce, &args).await,
			"SUBSCRIBE" | "UNSUBSCRIBE" => match payload.get("evt").and_then(Value::as_str) {
				Some(event) => match RpcEvent::parse(event) {
					Some(parsed) => {
						if name == "SUBSCRIBE" {
							self.shared.subscribe(self.id, parsed);
						} else {
							self.shared.unsubscribe(self.id, parsed);
						}
						reply(&cmd, &nonce, json!({ "evt": event }))
					}
					None => error(&cmd, &nonce, ERROR_INVALID_EVENT, "Invalid event"),
				},
				None => error(
					&cmd,
					&nonce,
					ERROR_INVALID_PAYLOAD,
					r#"child "evt" fails because ["evt" is required]"#,
				),
			},
			"SEND_ACTIVITY_JOIN_INVITE" => self.join_invite(&cmd, &nonce, &args),
			"CLOSE_ACTIVITY_REQUEST" | "CLOSE_ACTIVITY_JOIN_REQUEST" => {
				self.close_request(&cmd, &nonce, &args)
			}
			known if KNOWN_COMMANDS.contains(&known) => {
				error(&cmd, &nonce, ERROR_SERVICE_UNAVAILABLE, "Service unavailable")
			}
			_ => error(&cmd, &nonce, ERROR_INVALID_COMMAND, "Invalid command"),
		}
	}

	async fn set_activity(&mut self, cmd: &Value, nonce: &Value, args: &Value) -> Value {
		let Some(app) = self.app.clone() else {
			return error(cmd, nonce, ERROR_INVALID_PAYLOAD, "Not connected");
		};
		if self.transport == Transport::Ipc && !args.get("pid").is_some_and(Value::is_number) {
			return error(
				cmd,
				nonce,
				ERROR_INVALID_PAYLOAD,
				r#"child "pid" fails because ["pid" must be a number]"#,
			);
		}
		let raw = args.get("activity").filter(|activity| !activity.is_null());
		let Some(raw) = raw else {
			self.publish(None);
			return reply(cmd, nonce, Value::Null);
		};
		match activity::build(&app, &self.shared.apps, raw).await {
			Ok(built) => {
				self.publish(Some(built.clone()));
				reply(cmd, nonce, built)
			}
			Err(message) => error(cmd, nonce, ERROR_INVALID_PAYLOAD, message),
		}
	}

	fn join_invite(&self, cmd: &Value, nonce: &Value, args: &Value) -> Value {
		let Some(app) = &self.app else {
			return error(cmd, nonce, ERROR_INVALID_PAYLOAD, "Not connected");
		};
		let Some(user_id) = requesting_user(args) else {
			return invalid_user_id(cmd, nonce);
		};
		let Some(party_id) = self.party_id.clone() else {
			return error(cmd, nonce, ERROR_INVALID_PAYLOAD, "No party to invite to");
		};
		if !self.shared.take_pending(app.id.get(), user_id) {
			return error(cmd, nonce, ERROR_INVALID_PAYLOAD, "No pending join request");
		}
		let _ = self.shared.requests.send(ActivityRequest::JoinInvite {
			application_id: app.id,
			user_id,
			party_id,
		});
		reply(cmd, nonce, Value::Null)
	}

	fn close_request(&self, cmd: &Value, nonce: &Value, args: &Value) -> Value {
		let Some(app) = &self.app else {
			return error(cmd, nonce, ERROR_INVALID_PAYLOAD, "Not connected");
		};
		let Some(user_id) = requesting_user(args) else {
			return invalid_user_id(cmd, nonce);
		};
		self.shared.take_pending(app.id.get(), user_id);
		reply(cmd, nonce, Value::Null)
	}

	fn publish(&mut self, activity: Option<Value>) {
		self.has_activity = activity.is_some();
		self.party_id = activity
			.as_ref()
			.and_then(|activity| activity["party"]["id"].as_str())
			.map(str::to_owned);
		self.shared.set_party(self.id, self.party_id.clone());
		let _ = self.shared.inputs.send(Input::Rpc {
			link: self.id,
			activity,
		});
	}
}

impl Drop for Link {
	fn drop(&mut self) {
		self.shared.forget(self.id);
		if self.has_activity {
			let _ = self.shared.inputs.send(Input::Rpc {
				link: self.id,
				activity: None,
			});
		}
	}
}
