mod bitmap;
mod crypto;
mod flow;
mod messages;

use tokio::sync::mpsc::{self, Receiver};
use tokio::task::JoinHandle;
use url::Url;

pub use bitmap::QrBitmap;

const EVENT_BUFFER: usize = 8;
const GATEWAY_URL: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
const ORIGIN: &str = "https://discord.com";
const REST_BASE: &str = "https://discord.com/api/v9";

#[derive(Debug, Clone)]
pub struct QrConfig {
	pub gateway_url: Url,
	pub origin: String,
	pub rest_base: Url,
}

impl Default for QrConfig {
	fn default() -> Self {
		Self {
			gateway_url: Url::parse(GATEWAY_URL).expect("default gateway url is valid"),
			origin: ORIGIN.to_owned(),
			rest_base: Url::parse(REST_BASE).expect("default rest base is valid"),
		}
	}
}

#[derive(Debug)]
pub enum QrEvent {
	Code {
		url: String,
		bitmap: QrBitmap,
	},
	Scanned {
		username: String,
		avatar: Option<String>,
		id: String,
	},
	Token(protocol::Token),
	Expired,
	Cancelled,
	Error(String),
}

pub struct QrLogin;

impl QrLogin {
	pub fn start(config: QrConfig) -> QrSession {
		let (sender, receiver) = mpsc::channel(EVENT_BUFFER);
		QrSession {
			events: Some(receiver),
			task: tokio::spawn(flow::drive(config, sender)),
		}
	}
}

pub struct QrSession {
	events: Option<Receiver<QrEvent>>,
	task: JoinHandle<()>,
}

impl QrSession {
	pub fn events(&mut self) -> Option<Receiver<QrEvent>> {
		self.events.take()
	}

	pub fn abort(&self) {
		self.task.abort();
	}
}

impl Drop for QrSession {
	fn drop(&mut self) {
		self.task.abort();
	}
}
