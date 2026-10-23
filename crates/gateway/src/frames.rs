use protocol::gateway::Opcode;
use protocol::{IdentifyProperties, Token};
use serde::Serialize;
use serde_json::{Value, json};

use crate::config::GatewayConfig;
use crate::error::GatewayError;

#[derive(Serialize)]
struct Outgoing<'a, T: Serialize> {
	op: Opcode,
	d: &'a T,
}

#[derive(Serialize)]
struct IdentifyData<'a> {
	token: &'a Token,
	capabilities: u64,
	properties: &'a IdentifyProperties,
	presence: Presence<'a>,
	compress: bool,
	large_threshold: u32,
	client_state: ClientState,
}

#[derive(Serialize)]
struct Presence<'a> {
	status: &'a str,
	since: Option<u64>,
	activities: [Value; 0],
	afk: bool,
}

#[derive(Serialize)]
struct ClientState {
	guild_versions: serde_json::Map<String, Value>,
}

#[derive(Serialize)]
struct ResumeData<'a> {
	token: &'a Token,
	session_id: &'a str,
	seq: u64,
}

fn encode<T: Serialize>(opcode: Opcode, data: &T) -> Result<String, GatewayError> {
	Ok(serde_json::to_string(&Outgoing { op: opcode, d: data })?)
}

pub(crate) fn identify(config: &GatewayConfig) -> Result<String, GatewayError> {
	encode(
		Opcode::Identify,
		&IdentifyData {
			token: &config.token,
			capabilities: config.capabilities,
			properties: &config.identify,
			presence: Presence {
				status: &config.initial_status,
				since: None,
				activities: [],
				afk: false,
			},
			compress: false,
			large_threshold: config.large_threshold,
			client_state: ClientState {
				guild_versions: serde_json::Map::new(),
			},
		},
	)
}

pub(crate) fn resume(token: &Token, session_id: &str, seq: u64) -> Result<String, GatewayError> {
	encode(
		Opcode::Resume,
		&ResumeData {
			token,
			session_id,
			seq,
		},
	)
}

pub(crate) fn heartbeat(seq: Option<u64>) -> Result<String, GatewayError> {
	encode(Opcode::Heartbeat, &seq)
}

pub(crate) fn command(opcode: Opcode, data: Value) -> String {
	json!({ "op": i64::from(opcode), "d": data }).to_string()
}
