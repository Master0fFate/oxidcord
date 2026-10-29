use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use activity::{ActivityHandle, Engine, EngineConfig, RpcEndpoints};
use gateway::{Gateway, GatewayConfig, GatewayHandle};
use media::MediaFetcher;
use plugin_host::catalog::{Catalog, ThemeCatalog};
use plugin_host::{
	CommandEntry, Grants, Keybinds, Mounts, Patches, PluginInfo, PluginManager, RuntimeConfig, Slots, Watches,
};
use protocol::Token;
use protocol::gateway::IdentifyProperties;
use rest::{RestClient, RestConfig};
use state::{SharedState, State};
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::sync::mpsc;
use url::Url;
use voice::video::VideoSink;

use crate::broker::{BrokerCaches, SessionBroker, VoiceCache};
use crate::catalog::catalog_key;
use crate::command::Command;
use crate::config::{PluginOptions, SessionConfig, VoiceIo};
use crate::loads::PluginLoads;
use crate::marks::ClientMarks;
use crate::notifier::Notifier;
use crate::plugins::PluginHub;
use crate::search::SearchResults;
use crate::task::{Core, CoreParts, Internal, SharedSearch, SharedVideo, SharedVoice, Stats};

const COMMAND_CAPACITY: usize = 256;
const RUNTIME_WORKERS: usize = 4;
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);
const RUNTIME_GRACE: Duration = Duration::from_secs(2);
const PLUGIN_MEDIA_MAX_BYTES: usize = 8 * 1024 * 1024;
const SAVE_MEDIA_MAX_BYTES: usize = 100 * 1024 * 1024;

pub struct Session {
	state: SharedState,
	commands: mpsc::Sender<Command>,
	gateway: GatewayHandle,
	search: SharedSearch,
	#[cfg_attr(not(feature = "mock"), allow(dead_code))]
	voice: SharedVoice,
	video: SharedVideo,
	stats: Arc<Stats>,
	hub: Arc<PluginHub>,
	activity: ActivityHandle,
	finished: std::sync::mpsc::Receiver<()>,
	runtime: Option<Runtime>,
}

fn install_crypto_provider() {
	let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn media_fetcher(identify: &IdentifyProperties, rest_base: &Url, max_bytes: usize) -> Option<MediaFetcher> {
	let mut builder = MediaFetcher::builder()
		.user_agent(identify.user_agent())
		.max_bytes(max_bytes);
	if rest_base.host_str() == Some("127.0.0.1")
		&& let Some(port) = rest_base.port()
	{
		builder = builder.allow_loopback_http(port);
	}
	match builder.build() {
		Ok(fetcher) => Some(fetcher),
		Err(error) => {
			tracing::warn!(%error, "plugin media fetcher is unavailable");
			None
		}
	}
}

fn spawn_activity(
	rest: &RestClient,
	rest_base: Url,
	user_agent: String,
	store: &store::Store,
	handle: &Handle,
	internal: mpsc::UnboundedSender<Internal>,
) -> ActivityHandle {
	let (output, mut activities) = mpsc::unbounded_channel();
	handle.spawn(async move {
		while let Some(list) = activities.recv().await {
			if internal.send(Internal::Activities(list)).is_err() {
				break;
			}
		}
	});
	Engine::spawn(EngineConfig {
		rest: rest.clone(),
		rest_base,
		user_agent,
		cache_dir: store.paths.cache_dir().to_path_buf(),
		runtime: handle.clone(),
		output,
	})
}

fn runtime_for(requested: Option<Handle>) -> (Option<Runtime>, Handle) {
	match requested {
		Some(handle) => (None, handle),
		None => {
			let runtime = Builder::new_multi_thread()
				.worker_threads(RUNTIME_WORKERS)
				.thread_name("oxidcord-session")
				.enable_all()
				.build()
				.expect("session runtime initializes");
			let handle = runtime.handle().clone();
			(Some(runtime), handle)
		}
	}
}

impl Session {
	pub fn start(config: SessionConfig) -> Session {
		Self::start_with(config, VoiceIo::default())
	}

	pub fn start_with(config: SessionConfig, voice_io: VoiceIo) -> Session {
		Self::start_with_plugins(config, voice_io, PluginOptions::default())
	}

	pub fn start_with_plugins(config: SessionConfig, voice_io: VoiceIo, plugins: PluginOptions) -> Session {
		install_crypto_provider();
		let (runtime, handle) = runtime_for(config.runtime);
		let locale = config.locale.clone();
		let marks = Arc::new(ClientMarks::new());
		let loads = Arc::new(PluginLoads::default());
		let media = media_fetcher(&config.identify, &config.rest_base, PLUGIN_MEDIA_MAX_BYTES);
		let save_media = media_fetcher(&config.identify, &config.rest_base, SAVE_MEDIA_MAX_BYTES);
		let rest_base = config.rest_base.clone();
		let user_agent = config.identify.user_agent().to_owned();
		let rest = RestClient::new(
			RestConfig {
				base_url: config.rest_base,
				locale: config.locale,
				..RestConfig::default()
			}
			.with_identity(&config.identify),
		);
		rest.set_token(Token::new(config.token.as_str()));
		let token = Arc::new(Token::new(config.token.as_str()));
		let mut gateway_config = GatewayConfig::new(config.token, config.identify);
		gateway_config.gateway_url = config.gateway_url;
		let (gateway, gateway_events) = Gateway::spawn(gateway_config, handle.clone());
		let state = State::shared();
		let search = SharedSearch::default();
		let voice = SharedVoice::default();
		let video = SharedVideo::default();
		let stats = Arc::new(Stats::default());
		let (commands, command_receiver) = mpsc::channel(COMMAND_CAPACITY);
		let (internal, internal_receiver) = mpsc::unbounded_channel();
		let activity = spawn_activity(
			&rest,
			rest_base,
			user_agent,
			&config.store,
			&handle,
			internal.clone(),
		);
		let voice_cache = Arc::new(VoiceCache::default());
		let broker = Arc::new(SessionBroker {
			state: state.clone(),
			voice: voice_cache.clone(),
			commands: commands.clone(),
			internal: internal.clone(),
			sink: Arc::clone(&plugins.sink),
			handle: handle.clone(),
			marks: Arc::clone(&marks),
			loads: Arc::clone(&loads),
			locale: locale.clone(),
			caches: BrokerCaches::default(),
		});
		let grants = Arc::new(Grants::new(config.store.clone()));
		let plugin_runtime = plugin_host::Runtime::new(
			RuntimeConfig {
				catalog_key: catalog_key(),
				..RuntimeConfig::default()
			},
			handle.clone(),
			config.store.clone(),
			grants.clone(),
			broker.clone(),
		)
		.expect("plugin runtime initializes");
		let manager = Arc::new(PluginManager::new(plugin_runtime));
		broker.attach_plugins(&manager);
		let hub = Arc::new(PluginHub::new(manager, grants, config.store.clone()));
		let core = Core::new(CoreParts {
			state: state.clone(),
			rest,
			gateway: gateway.clone(),
			store: config.store,
			internal,
			hub: hub.clone(),
			safe_mode: plugins.safe_mode,
			locale,
			notifier: Notifier::new(config.notify),
			runtime: handle.clone(),
			search: search.clone(),
			stats: stats.clone(),
			voice_cache,
			voice_io,
			shared_voice: voice.clone(),
			video: video.clone(),
			token,
			loads,
			marks,
			media,
			save_media,
			sink: plugins.sink,
			activity: activity.clone(),
		});
		let (done, finished) = std::sync::mpsc::channel();
		handle.spawn(async move {
			core.run(gateway_events, command_receiver, internal_receiver)
				.await;
			let _ = done.send(());
		});
		Session {
			state,
			commands,
			gateway,
			search,
			voice,
			video,
			stats,
			hub,
			activity,
			finished,
			runtime,
		}
	}

	pub fn rpc_endpoints(&self) -> RpcEndpoints {
		self.activity.endpoints()
	}

	pub fn state(&self) -> &SharedState {
		&self.state
	}

	pub fn send(&self, command: Command) {
		if let Err(error) = self.commands.try_send(command) {
			tracing::warn!(%error, "session command was not queued");
		}
	}

	pub fn plugins(&self) -> &Arc<PluginManager> {
		&self.hub.manager
	}

	pub fn plugin_slots(&self) -> Arc<Slots> {
		Arc::clone(self.hub.manager.slots())
	}

	pub fn plugin_mounts(&self) -> Arc<Mounts> {
		Arc::clone(self.hub.manager.runtime().mounts())
	}

	pub fn plugin_patches(&self) -> Arc<Patches> {
		Arc::clone(self.hub.manager.runtime().patches())
	}

	pub fn plugin_keybinds(&self) -> Arc<Keybinds> {
		Arc::clone(self.hub.manager.runtime().keybinds())
	}

	pub fn plugin_watches(&self) -> Arc<Watches> {
		Arc::clone(self.hub.manager.runtime().watches())
	}

	pub fn plugin_commands(&self) -> Arc<[(String, CommandEntry)]> {
		self.hub.manager.commands()
	}

	pub fn plugin_list(&self) -> Vec<PluginInfo> {
		self.hub.manager.list()
	}

	pub fn plugin_error(&self) -> Option<String> {
		self.hub.last_error()
	}

	pub fn plugin_render_ms_p95(&self) -> Option<f64> {
		self.hub.stats.p95_ms()
	}

	pub fn plugin_call_ms_p95(&self) -> Option<f64> {
		self.hub.manager.interactive_call_p95_ms()
	}

	pub fn catalog(&self) -> Option<Arc<Catalog>> {
		self.hub.catalog()
	}

	pub fn catalog_error(&self) -> Option<String> {
		self.hub.catalog_error()
	}

	pub fn theme_catalog(&self) -> Option<Arc<ThemeCatalog>> {
		self.hub.theme_catalog()
	}

	pub fn theme_catalog_error(&self) -> Option<String> {
		self.hub.theme_catalog_error()
	}

	pub fn theme_files(&self) -> Vec<std::path::PathBuf> {
		self.hub.theme_files()
	}

	pub fn search_results(&self) -> Option<Arc<SearchResults>> {
		self.search.lock().clone()
	}

	#[cfg(feature = "mock")]
	pub fn push_voice_test_frame(&self, pcm: &[i16]) -> bool {
		self.voice
			.lock()
			.as_ref()
			.map(|connection| connection.push_test_frame(pcm))
			.is_some()
	}

	#[cfg(feature = "mock")]
	pub fn take_voice_output_frames(&self) -> Vec<Vec<i16>> {
		self.voice
			.lock()
			.as_ref()
			.map(|connection| connection.take_output_frames())
			.unwrap_or_default()
	}

	pub fn set_video_sink(&self, sink: Arc<dyn VideoSink>) {
		self.video.lock().set_sink(sink);
	}

	pub fn reconnects(&self) -> u64 {
		self.stats.reconnects.load(Ordering::Relaxed)
	}

	pub fn shutdown(mut self) {
		self.gateway.shutdown();
		let _ = self.commands.try_send(Command::Shutdown);
		if self.finished.recv_timeout(SHUTDOWN_WAIT).is_err() {
			tracing::warn!("session did not stop in time");
		}
		if let Some(runtime) = self.runtime.take() {
			runtime.shutdown_timeout(RUNTIME_GRACE);
		}
	}
}

impl Drop for Session {
	fn drop(&mut self) {
		self.gateway.shutdown();
		let _ = self.commands.try_send(Command::Shutdown);
		if let Some(runtime) = self.runtime.take() {
			runtime.shutdown_background();
		}
	}
}
