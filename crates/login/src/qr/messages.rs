use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(super) enum Incoming {
	Hello {
		heartbeat_interval: u64,
		timeout_ms: u64,
	},
	HeartbeatAck,
	NonceProof {
		encrypted_nonce: String,
	},
	PendingRemoteInit {
		fingerprint: String,
	},
	PendingTicket {
		encrypted_user_payload: String,
	},
	PendingLogin {
		ticket: String,
	},
	Cancel,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(super) enum Outgoing<'a> {
	Init { encoded_public_key: &'a str },
	Heartbeat,
	NonceProof { nonce: &'a str },
}

#[derive(Debug, Serialize)]
pub(super) struct TicketExchange<'a> {
	pub ticket: &'a str,
}

#[derive(Debug, Deserialize)]
pub(super) struct TokenGrant {
	pub encrypted_token: String,
}
