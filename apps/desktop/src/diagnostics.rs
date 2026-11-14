use std::fmt::{self, Write as _};
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use tracing::field::{Field, Visit};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const LIMIT: u64 = 10 * 1024 * 1024;

pub struct Recorder {
	start: Instant,
	state: Mutex<State>,
}

struct State {
	file: Option<File>,
	bytes: u64,
	seq: u64,
}

impl Recorder {
	pub fn new(logs: &Path, mock: bool, safe_mode: bool) -> Option<Self> {
		let start = Instant::now();
		let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis();
		std::fs::create_dir_all(logs).ok()?;
		let file = OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(logs.join(format!("diagnostics-{timestamp}-{}.jsonl", std::process::id())))
			.ok()?;
		let recorder = Self {
			start,
			state: Mutex::new(State {
				file: Some(file),
				bytes: 0,
				seq: 0,
			}),
		};
		let mut fields = Map::new();
		fields.insert("format_version".into(), json!(1));
		fields.insert("app_version".into(), json!(env!("CARGO_PKG_VERSION")));
		fields.insert("os".into(), json!(std::env::consts::OS));
		fields.insert("startup_unix_ms".into(), json!(timestamp));
		fields.insert("mock".into(), json!(mock));
		fields.insert("safe_mode".into(), json!(safe_mode));
		recorder.write("header", fields);
		recorder.state.lock().ok()?.file.as_ref()?;
		Some(recorder)
	}

	fn write(&self, event: &'static str, mut fields: Map<String, Value>) {
		let Ok(mut state) = self.state.lock() else { return };
		if state.file.is_none() {
			return;
		}
		let elapsed = self.start.elapsed().as_millis().min(u64::MAX as u128) as u64;
		fields.insert("event".into(), json!(event));
		fields.insert("seq".into(), json!(state.seq));
		fields.insert("elapsed_ms".into(), json!(elapsed));
		let Ok(mut line) = serde_json::to_vec(&fields) else {
			return;
		};
		line.push(b'\n');
		let at_limit = state.bytes + line.len() as u64 + 128 > LIMIT;
		if at_limit {
			line = json!({"event": "capture_limit", "seq": state.seq, "elapsed_ms": elapsed})
				.to_string()
				.into_bytes();
			line.push(b'\n');
		}
		if state.bytes + line.len() as u64 > LIMIT
			|| state
				.file
				.as_mut()
				.is_none_or(|file| file.write_all(&line).is_err())
		{
			state.file = None;
			eprintln!("Oxidcord stopped recording diagnostics because the file could not be written.");
			return;
		}
		state.bytes += line.len() as u64;
		state.seq += 1;
		if at_limit {
			state.file = None;
		}
	}
}

pub fn allowed_metadata(metadata: &Metadata<'_>) -> bool {
	metadata.is_event()
		&& matches!(
			metadata.target(),
			"login::web_login"
				| "ui::shell::login"
				| "ui::shell::login::web"
				| "gateway::session"
				| "session::task"
				| "session::task::actions"
				| "session::task::commands"
				| "rest::client"
		)
}

impl<S: Subscriber> Layer<S> for Recorder {
	fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
		let mut message = Message {
			target: event.metadata().target(),
			event: None,
		};
		event.record(&mut message);
		let Some(name) = message.event else { return };
		let mut fields = Fields {
			event: name,
			values: Map::new(),
		};
		event.record(&mut fields);
		self.write(name, fields.values);
	}
}

fn event_name(target: &str, message: &str) -> Option<&'static str> {
	Some(match (target, message) {
		("login::web_login", "web login handoff received") => "web_login_handoff_received",
		("login::web_login", "web login handoff exchanged") => "web_login_handoff_exchanged",
		("login::web_login", "web login helper started") => "web_login_helper_started",
		("login::web_login", "web login handoff exchange started") => "web_login_exchange_started",
		("login::web_login", "web login handoff exchange failed") => "web_login_exchange_failed",
		("ui::shell::login", "login REST validation finished") => "login_validation_finished",
		("ui::shell::login::web", "web login requested") => "web_login_requested",
		("ui::shell::login::web", "web login cancelled") => "web_login_cancelled",
		("ui::shell::login::web", "web login finished") => "web_login_finished",
		("gateway::session", "gateway connecting") => "gateway_connecting",
		("gateway::session", "gateway authentication sending") => "gateway_authentication_sending",
		("gateway::session", "gateway authentication sent") => "gateway_authentication_sent",
		("gateway::session", "gateway READY received") => "gateway_ready",
		("gateway::session", "gateway token rotated") => "gateway_token_rotated",
		("gateway::session", "gateway resumed") => "gateway_resumed",
		("gateway::session", "gateway invalid session") => "gateway_invalid_session",
		("gateway::session", "gateway connection ended") => "gateway_connection_ended",
		("gateway::session", "gateway reconnecting") => "gateway_reconnecting",
		("gateway::session", "gateway stopped") => "gateway_stopped",
		("session::task", "gateway disconnected") => "gateway_disconnected",
		("session::task", "authentication rejected") => "authentication_rejected",
		("session::task::actions", "rest call failed") => "rest_call_failed",
		("session::task::actions", "discord rejected the session token") => "session_token_rejected",
		("session::task::commands", "invite preview unavailable") => "invite_preview_unavailable",
		("rest::client", "rest response") => "rest_response",
		("rest::client", "rest rate limited, retrying") => "rest_rate_limited",
		_ => return None,
	})
}

struct Bounded {
	text: String,
}

impl fmt::Write for Bounded {
	fn write_str(&mut self, value: &str) -> fmt::Result {
		if self.text.len() + value.len() > 128 {
			return Err(fmt::Error);
		}
		self.text.push_str(value);
		Ok(())
	}
}

fn render(value: &dyn fmt::Debug) -> Option<String> {
	let mut output = Bounded { text: String::new() };
	write!(&mut output, "{value:?}").ok()?;
	Some(output.text)
}

struct Message<'a> {
	target: &'a str,
	event: Option<&'static str>,
}

impl Visit for Message<'_> {
	fn record_str(&mut self, field: &Field, value: &str) {
		if field.name() == "message" {
			self.event = event_name(self.target, value);
		}
	}

	fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
		if field.name() == "message"
			&& let Some(value) = render(value)
		{
			self.event = event_name(self.target, &value);
		}
	}
}

struct Fields {
	event: &'static str,
	values: Map<String, Value>,
}

impl Fields {
	fn kind(&self, name: &str) -> Option<&'static str> {
		Some(match (self.event, name) {
			("gateway_connecting", "attempt") | ("rest_rate_limited", "retries") => "number",
			("gateway_connection_ended" | "gateway_reconnecting" | "gateway_disconnected", "code") => {
				"number"
			}
			(
				"web_login_exchange_failed"
				| "login_validation_finished"
				| "rest_call_failed"
				| "invite_preview_unavailable",
				"status" | "api_code",
			) => "number",
			("rest_response", "status" | "elapsed_ms" | "remaining") => "number",
			("login_validation_finished" | "web_login_finished", "success")
			| ("web_login_finished", "cancelled")
			| ("gateway_token_rotated", "changed")
			| ("gateway_invalid_session", "resumable" | "will_resume")
			| ("gateway_connection_ended", "ready_received" | "before_auth")
			| ("gateway_reconnecting" | "gateway_disconnected", "will_resume")
			| ("rest_rate_limited", "global") => "bool",
			(
				"gateway_connecting" | "gateway_authentication_sending" | "gateway_authentication_sent",
				"mode",
			) => "mode",
			("authentication_rejected", "source") => "source",
			("web_login_exchange_failed", "failure") => "failure",
			("rest_call_failed", "what") => "what",
			("rest_response", "method") => "method",
			_ => return None,
		})
	}

	fn insert(&mut self, name: &str, value: Value) {
		let key = if self.event == "rest_response" && name == "elapsed_ms" {
			"request_elapsed_ms"
		} else {
			name
		};
		self.values.insert(key.to_owned(), value);
	}

	fn number(&mut self, name: &str, value: u64) {
		let valid = match name {
			"status" => (100..=599).contains(&value),
			"code" => value <= u64::from(u16::MAX),
			_ => value <= u64::from(u32::MAX),
		};
		if valid {
			self.insert(name, json!(value));
		}
	}

	fn string(&mut self, name: &str, value: &str) {
		let Some(kind) = self.kind(name) else { return };
		let value = value
			.strip_prefix('"')
			.and_then(|v| v.strip_suffix('"'))
			.unwrap_or(value);
		let safe = match kind {
			"mode" => match value {
				"identify" => "identify",
				"resume" => "resume",
				_ => return,
			},
			"source" => match value {
				"rest" => "rest",
				"gateway" => "gateway",
				_ => return,
			},
			"failure" => match value {
				"network" => "network",
				"captcha" => "captcha",
				"rejected" => "rejected",
				"protocol" => "protocol",
				"other" => "other",
				_ => return,
			},
			"what" => match value {
				"invite" => "invite",
				"invite preview" => "invite preview",
				"gif favorites" => "gif favorites",
				"ack" => "ack",
				_ => "other",
			},
			"method" => match value {
				"GET" => "GET",
				"POST" => "POST",
				"PUT" => "PUT",
				"PATCH" => "PATCH",
				"DELETE" => "DELETE",
				"HEAD" => "HEAD",
				"OPTIONS" => "OPTIONS",
				_ => return,
			},
			"number" => {
				let number = value
					.strip_prefix("Some(")
					.and_then(|v| v.strip_suffix(')'))
					.unwrap_or(value);
				if let Ok(number) = number.parse::<u64>() {
					self.number(name, number);
				}
				return;
			}
			_ => return,
		};
		self.insert(name, json!(safe));
	}
}

impl Visit for Fields {
	fn record_u64(&mut self, field: &Field, value: u64) {
		if self.kind(field.name()) == Some("number") {
			self.number(field.name(), value);
		}
	}

	fn record_i64(&mut self, field: &Field, value: i64) {
		if value >= 0 {
			self.record_u64(field, value as u64);
		}
	}

	fn record_bool(&mut self, field: &Field, value: bool) {
		if self.kind(field.name()) == Some("bool") {
			self.insert(field.name(), json!(value));
		}
	}

	fn record_str(&mut self, field: &Field, value: &str) {
		if self.kind(field.name()).is_some() {
			self.string(field.name(), value);
		}
	}

	fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
		if self.kind(field.name()).is_some()
			&& let Some(value) = render(value)
		{
			self.string(field.name(), &value);
		}
	}
}
