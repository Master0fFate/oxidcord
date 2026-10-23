use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
	#[error("gateway rejected the session: {}", fatal_meaning(*.code))]
	Fatal { code: u16 },
	#[error("gateway url is not valid")]
	InvalidUrl,
	#[error("gateway url must use wss, or ws on a loopback host")]
	InsecureUrl,
	#[error("incoming gateway data exceeded the size cap")]
	OverSized,
	#[error("gateway zlib stream is corrupt")]
	Decompress,
	#[error("failed to encode an outgoing frame: {0}")]
	Encode(#[from] serde_json::Error),
	#[error("websocket error: {0}")]
	Transport(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SendError {
	#[error("gateway send queue is full")]
	Full,
	#[error("gateway session has stopped")]
	Closed,
}

pub(crate) const FATAL_CLOSE_CODES: [u16; 6] = [4004, 4010, 4011, 4012, 4013, 4014];

fn fatal_meaning(code: u16) -> &'static str {
	match code {
		4004 => "authentication failed",
		4010 => "invalid shard",
		4011 => "sharding required",
		4012 => "invalid api version",
		4013 => "invalid intents",
		4014 => "disallowed intents",
		_ => "fatal close code",
	}
}
