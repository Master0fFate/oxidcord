use std::time::Duration;

use protocol::gateway::{DispatchEvent, Ready, ReadySupplemental};

use crate::error::GatewayError;

#[derive(Debug)]
pub enum GatewayEvent {
	Connecting {
		attempt: u32,
	},
	Hello {
		heartbeat_interval: Duration,
	},
	Ready(Box<Ready>),
	ReadySupplemental(Box<ReadySupplemental>),
	Resumed,
	Dispatch {
		seq: u64,
		event: DispatchEvent,
	},
	Disconnected {
		code: Option<u16>,
		reason: String,
		will_resume: bool,
	},
	Fatal(GatewayError),
	Latency {
		heartbeat_rtt: Duration,
	},
	DecodeFailure {
		event: String,
	},
}
