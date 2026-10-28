#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::Parser;
use mockcord::{MockConfig, Server};
use platform::notifications::Notification;
use platform::single_instance::{self, AlreadyRunning, Guard};
use protocol::Token;
use protocol::gateway::IdentifyProperties;
use session::{BrokerSink, PluginOptions, Session, SessionConfig, VoiceIo};
use store::{Paths, Store};
use tokio::runtime::Handle;
use tracing_subscriber::EnvFilter;
use ui::{AppFlags, LaunchConfig, StartSession};
use url::Url;

const MOCK_PROFILE: &str = "mock-profile";
const BUILD_REFRESH_INTERVAL_SECS: i64 = 24 * 60 * 60;
const BUILD_REFRESH_TIMEOUT: Duration = Duration::from_secs(10);
const MOCK_QR_GATEWAY: &str = "ws://127.0.0.1:9/";
const RELAUNCH_ATTEMPTS: u32 = 100;
const RELAUNCH_RETRY: Duration = Duration::from_millis(100);

#[derive(Parser)]
#[command(name = "oxidcord", version, about = "A native Discord client")]
struct Cli {
	#[arg(long, help = "Run against the mock Discord server")]
	mock: bool,
	#[arg(
		long,
		default_value_t = 1,
		help = "Seed for the mock server fixtures, used with --mock"
	)]
	seed: u64,
	#[arg(
		long,
		help = "Make the mock server drop connections and rate limit, used with --mock"
	)]
	chaos: bool,
	#[arg(
		long,
		help = "Make the mock server send messages and typing on its own, used with --mock"
	)]
	live_traffic: bool,
	#[arg(long, hide = true, help = "Wait for the previous instance to exit")]
	relaunch: bool,
	#[arg(long, help = "Start without plugins")]
	safe_mode: bool,
	#[arg(long, hide = true, help = "Show the login screen even with --mock")]
	login: bool,
	#[arg(
		long,
		hide = true,
		value_name = "PATH",
		help = "Executable to run as the web login helper"
	)]
	web_login_helper: Option<PathBuf>,
	#[arg(
		long,
		value_name = "PATH",
		help = "Store settings, cache and logs under this directory"
	)]
	data_dir: Option<PathBuf>,
	#[arg(
		long,
		value_name = "FILTER",
		default_value = "info",
		help = "Log filter, for example debug or ui=trace"
	)]
	log: String,
}

struct PlatformSink;

impl BrokerSink for PlatformSink {
	fn clipboard_write(&self, plugin: &str, text: &str) -> bool {
		report(plugin, "clipboard", platform::clipboard::write_text(text))
	}

	fn notify(&self, plugin: &str, title: &str, body: &str) -> bool {
		let notification = Notification {
			title: title.to_owned(),
			body: body.to_owned(),
			icon_path: None,
			app_id: ui::APP_ID.to_owned(),
		};
		report(
			plugin,
			"notification",
			platform::notifications::show(&notification),
		)
	}

	fn open_url(&self, plugin: &str, url: &str) -> bool {
		report(plugin, "open url", platform::open::open_url(url))
	}

	fn idle_seconds(&self) -> Option<u64> {
		platform::idle::seconds_since_input()
	}

	fn speak(&self, _plugin: &str, text: &str, rate: f32, volume: f32) -> bool {
		platform::speech::speak_with(text, rate, volume)
	}
}

fn report(plugin: &str, what: &str, outcome: platform::Result<()>) -> bool {
	match outcome {
		Ok(()) => true,
		Err(error) => {
			tracing::warn!(plugin, %error, "plugin {what} request failed");
			false
		}
	}
}

struct Endpoints {
	rest_base: Url,
	gateway_url: String,
	token: Option<String>,
	mock_origin: Option<Url>,
}

fn main() -> anyhow::Result<()> {
	if let Some(code) = run_login_helper() {
		std::process::exit(code);
	}
	let cli = Cli::parse();
	init_tracing(&cli.log);

	let paths = match (&cli.data_dir, cli.mock) {
		(Some(dir), _) => Paths::at(dir),
		(None, true) => Paths::discover().and_then(|live| Paths::at(live.data_dir().join(MOCK_PROFILE))),
		(None, false) => Paths::discover(),
	}
	.context("prepare data directories")?;
	match updater::apply_pending_on_start(paths.data_dir()) {
		Ok(outcome) if outcome.should_exit() => return Ok(()),
		Ok(_) => {}
		Err(error) => tracing::warn!(%error, "could not apply the pending update"),
	}

	if let Err(error) = platform::notifications::set_app_user_model_id(ui::APP_ID) {
		tracing::warn!(%error, "could not set the app user model id");
	}

	let mut guard = match acquire_instance(cli.relaunch) {
		Ok(guard) => guard,
		Err(AlreadyRunning) => {
			forward_arguments();
			return Ok(());
		}
	};
	let incoming = guard.incoming();

	let store = Arc::new(Store::open(paths).context("open store")?);

	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.thread_name("oxidcord-worker")
		.build()
		.context("start async runtime")?;

	let mock = if cli.mock {
		let server = runtime
			.block_on(Server::start(MockConfig {
				seed: cli.seed,
				chaos: cli.chaos,
				live_traffic: cli.live_traffic,
				..MockConfig::default()
			}))
			.context("start the mock server")?;
		Some(server)
	} else {
		None
	};
	let endpoints = endpoints(mock.as_ref(), &store, cli.login)?;
	if !cli.mock {
		refresh_client_identity(&runtime, &store);
	}

	let data_dir = store.paths.data_dir().to_path_buf();
	let runtime_guard = runtime.enter();
	ui::run(LaunchConfig {
		store: store.clone(),
		flags: AppFlags {
			mock: cli.mock,
			safe_mode: cli.safe_mode,
		},
		rest_base: endpoints.rest_base.clone(),
		mock_origin: endpoints.mock_origin,
		token: endpoints.token,
		start_session: session_starter(&store, endpoints.rest_base, endpoints.gateway_url, cli.safe_mode),
		qr_config: qr_config(cli.mock),
		web_login_helper: cli.web_login_helper,
		activation: Some(incoming),
		on_window_ready: Some(Box::new(move || {
			if let Err(error) = updater::mark_start_ok(&data_dir) {
				tracing::warn!(%error, "could not confirm the update start");
			}
		})),
	});
	drop(runtime_guard);

	if let Some(server) = mock {
		runtime.block_on(server.shutdown());
	}
	Ok(())
}

fn acquire_instance(relaunched: bool) -> Result<Guard, AlreadyRunning> {
	if relaunched {
		for _ in 1..RELAUNCH_ATTEMPTS {
			if let Ok(guard) = single_instance::acquire(ui::APP_ID) {
				return Ok(guard);
			}
			std::thread::sleep(RELAUNCH_RETRY);
		}
	}
	single_instance::acquire(ui::APP_ID)
}

fn endpoints(mock: Option<&Server>, store: &Store, show_login: bool) -> anyhow::Result<Endpoints> {
	if let Some(server) = mock {
		return Ok(Endpoints {
			rest_base: Url::parse(server.rest_base_url()).context("mock rest url")?,
			gateway_url: server.gateway_url().to_owned(),
			token: (!show_login).then(|| server.token().to_owned()),
			mock_origin: Some(Url::parse(&format!("http://{}/", server.address())).context("mock origin")?),
		});
	}
	let token = match store.tokens.load() {
		Ok(token) => token
			.map(|token| token.trim().to_owned())
			.filter(|token| !token.is_empty()),
		Err(error) => {
			tracing::warn!(%error, "could not read the saved token");
			None
		}
	};
	Ok(Endpoints {
		rest_base: rest::RestConfig::default().base_url,
		gateway_url: gateway::GatewayConfig::DEFAULT_GATEWAY_URL.to_owned(),
		token,
		mock_origin: None,
	})
}

fn session_starter(store: &Arc<Store>, rest_base: Url, gateway_url: String, safe_mode: bool) -> StartSession {
	let store = store.clone();
	let (locale, build_number, chrome_version) = {
		let settings = store.settings.read();
		(
			settings.locale.clone(),
			settings.client.build_number,
			settings.client.chrome_version.clone(),
		)
	};
	let runtime = Handle::current();
	let plugins = PluginOptions::new(safe_mode, Arc::new(PlatformSink));
	Box::new(move |token, notify| {
		Session::start_with_plugins(
			SessionConfig {
				store: store.clone(),
				rest_base: rest_base.clone(),
				gateway_url: gateway_url.clone(),
				token: Token::new(token),
				identify: IdentifyProperties::web_client(&locale, build_number, &chrome_version),
				locale: locale.clone(),
				runtime: Some(runtime.clone()),
				notify,
			},
			VoiceIo::Hardware,
			plugins.clone(),
		)
	})
}

fn refresh_client_identity(runtime: &tokio::runtime::Runtime, store: &Arc<Store>) {
	let now = unix_now();
	if now - store.settings.read().client.build_checked_at < BUILD_REFRESH_INTERVAL_SECS {
		return;
	}
	let store = store.clone();
	runtime.spawn(async move {
		let Ok(client) = rest::api::build::discovery_client() else {
			return;
		};
		let discovery = async {
			let build_number = rest::api::build::discover_build_number(&client).await.ok();
			let chrome_version = rest::api::build::discover_chrome_version(&client).await.ok();
			(build_number, chrome_version)
		};
		let Ok((build_number, chrome_version)) = tokio::time::timeout(BUILD_REFRESH_TIMEOUT, discovery).await
		else {
			return;
		};
		if build_number.is_none() && chrome_version.is_none() {
			return;
		}
		let saved = store.update_settings(|settings| {
			if let Some(number) = build_number {
				settings.client.build_number = number;
			}
			if let Some(version) = chrome_version {
				settings.client.chrome_version = version;
			}
			settings.client.build_checked_at = now;
		});
		if saved.is_ok() {
			tracing::debug!("discord client identity refreshed");
		}
	});
}

fn unix_now() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn init_tracing(filter: &str) {
	let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
	tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn run_login_helper() -> Option<i32> {
	let mut args = std::env::args();
	if args.nth(1).as_deref() != Some(login::web_login::HELPER_FLAG) {
		return None;
	}
	let ipc_name = args.next().unwrap_or_default();
	Some(match login::web_login::run_helper(&ipc_name) {
		Ok(()) => 0,
		Err(_) => 1,
	})
}

fn qr_config(mock: bool) -> login::QrConfig {
	if !mock {
		return login::QrConfig::default();
	}
	login::QrConfig {
		gateway_url: Url::parse(MOCK_QR_GATEWAY).expect("mock qr gateway url"),
		..login::QrConfig::default()
	}
}

fn forward_arguments() {
	let payload = std::env::args().skip(1).collect::<Vec<_>>().join("\n");
	if let Err(error) = single_instance::notify_existing(ui::APP_ID, payload.as_bytes()) {
		tracing::warn!(%error, "could not reach the running instance");
	}
}
