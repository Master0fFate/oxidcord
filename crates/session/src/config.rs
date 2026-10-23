use std::sync::Arc;

use protocol::Token;
use protocol::gateway::IdentifyProperties;
use state::Changes;
use tokio::runtime::Handle;
use url::Url;

use crate::broker::{BrokerSink, NoopSink};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceIo {
	#[default]
	Hardware,
	Silent,
}

pub type NotifyFn = Arc<dyn Fn(Changes) + Send + Sync>;

pub struct SessionConfig {
	pub store: Arc<store::Store>,
	pub rest_base: Url,
	pub gateway_url: String,
	pub token: Token,
	pub identify: IdentifyProperties,
	pub locale: String,
	pub runtime: Option<Handle>,
	pub notify: NotifyFn,
}

#[derive(Clone)]
pub struct PluginOptions {
	pub safe_mode: bool,
	pub sink: Arc<dyn BrokerSink>,
}

impl Default for PluginOptions {
	fn default() -> Self {
		Self {
			safe_mode: false,
			sink: Arc::new(NoopSink),
		}
	}
}

impl PluginOptions {
	pub fn new(safe_mode: bool, sink: Arc<dyn BrokerSink>) -> Self {
		Self { safe_mode, sink }
	}
}
