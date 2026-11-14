#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod diagnostics;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::Parser;
use mockcord::{MockConfig, Server};
use platform::notifications::Notification;
use platform::single_instance::{self, AlreadyRunning, Guard};
use protocol::Token;
use protocol::gateway::{IdentifyProperties, effective_chrome_version};
use session::{BrokerSink, PluginOptions, Session, SessionConfig, VoiceIo};
use store::{Paths, Store};
use tokio::runtime::Handle;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt as _;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
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
	#[arg(long, help = "Record local diagnostics")]
	diagnostics: bool,
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
		report(plugin, "open url", platform::open::open_plugin_url(url))
	}

	fn idle_seconds(&self) -> Option<u64> {
		platform::idle::seconds_since_input()
	}

	fn speak(&self, _plugin: &str, text: &str, rate: f32, volume: f32) -> bool {
		platform::speech::speak_with(text, rate, volume)
	}

	fn pick_files(&self, multiple: bool) -> Vec<PathBuf> {
		platform::dialog::pick_files(multiple)
	}

	fn save_file(&self, file_name: &str) -> Option<PathBuf> {
		platform::dialog::save_file(file_name)
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
	let paths = match (&cli.data_dir, cli.mock) {
		(Some(dir), _) => Paths::at(dir),
		(None, true) => Paths::discover().and_then(|live| Paths::at(live.data_dir().join(MOCK_PROFILE))),
		(None, false) => Paths::discover(),
	}
	.context("prepare data directories")?;
	init_tracing(
		&cli.log,
		&paths.logs_dir(),
		cli.diagnostics,
		cli.mock,
		cli.safe_mode,
	);
	install_panic_hook(paths.logs_dir());
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
	if let Some(server) = &mock {
		add_second_mock_account(&store, server);
	}
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

fn add_second_mock_account(store: &Store, server: &Server) {
	let user = &server.fixtures().second_user;
	if let Err(error) = store.accounts.save(
		user.id,
		&user.username,
		user.avatar.as_deref(),
		server.second_token(),
	) {
		tracing::warn!(%error, "could not add the second mock account");
	}
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

fn client_identity(store: &Store) -> (String, u64, String) {
	let settings = store.settings.read();
	(
		settings.locale.clone(),
		settings.client.build_number,
		effective_chrome_version(&settings.client.chrome_version),
	)
}

fn session_starter(store: &Arc<Store>, rest_base: Url, gateway_url: String, safe_mode: bool) -> StartSession {
	let store = store.clone();
	let (locale, build_number, chrome_version) = client_identity(&store);
	rest::install_identity(IdentifyProperties::web_client(
		&locale,
		build_number,
		&chrome_version,
	));
	let runtime = Handle::current();
	let plugins = PluginOptions::new(safe_mode, Arc::new(PlatformSink));
	Box::new(move |token, notify| {
		let (locale, build_number, chrome_version) = client_identity(&store);
		rest::install_identity(IdentifyProperties::web_client(
			&locale,
			build_number,
			&chrome_version,
		));
		Session::start_with_plugins(
			SessionConfig {
				store: store.clone(),
				rest_base: rest_base.clone(),
				gateway_url: gateway_url.clone(),
				token: Token::new(token),
				identify: IdentifyProperties::web_client(&locale, build_number, &chrome_version),
				locale,
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
			let settings = store.settings.read();
			rest::install_identity(IdentifyProperties::web_client(
				&settings.locale,
				settings.client.build_number,
				&effective_chrome_version(&settings.client.chrome_version),
			));
			tracing::debug!("discord client identity refreshed");
		}
	});
}

fn unix_now() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn init_tracing(filter: &str, logs: &Path, diagnostics: bool, mock: bool, safe_mode: bool) {
	use tracing_subscriber::Layer as _;
	use tracing_subscriber::filter::filter_fn;

	let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
	let recorder = diagnostics
		.then(|| diagnostics::Recorder::new(logs, mock, safe_mode))
		.flatten();
	let diagnostics_failed = diagnostics && recorder.is_none();
	let recorder = recorder.map(|recorder| recorder.with_filter(filter_fn(diagnostics::allowed_metadata)));
	let subscriber = tracing_subscriber::registry().with(recorder);
	let log = tracing_subscriber::fmt::layer().with_ansi(false);
	match log_file(logs) {
		Some(file) => subscriber
			.with(log.with_writer(std::io::stdout.and(file)).with_filter(filter))
			.init(),
		None => subscriber.with(log.with_filter(filter)).init(),
	}
	if diagnostics_failed {
		tracing::warn!("local diagnostics could not be started");
		eprintln!("Oxidcord could not start local diagnostics.");
	} else if diagnostics {
		tracing::info!(directory = %logs.display(), "local diagnostics recording");
	}
}

fn log_file(logs: &Path) -> Option<Arc<File>> {
	std::fs::create_dir_all(logs).ok()?;
	File::create(logs.join("session.log")).ok().map(Arc::new)
}

fn install_panic_hook(logs: PathBuf) {
	let default_hook = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		let message = info
			.payload()
			.downcast_ref::<&str>()
			.map(|text| (*text).to_owned())
			.or_else(|| info.payload().downcast_ref::<String>().cloned())
			.unwrap_or_default();
		let location = info
			.location()
			.map(|at| format!("{}:{}", at.file(), at.line()))
			.unwrap_or_default();
		tracing::error!(message, location, "panic");
		let line = format!(
			"{} {location} {message}
",
			unix_now()
		);
		let appended = std::fs::OpenOptions::new()
			.create(true)
			.append(true)
			.open(logs.join("panic.log"))
			.and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
		if let Err(error) = appended {
			tracing::warn!(%error, "could not write the panic log");
		}
		default_hook(info);
	}));
}

fn run_login_helper() -> Option<i32> {
	let mut args = std::env::args();
	let mode = args.nth(1)?;
	let code = |result: login::Result<()>| match result {
		Ok(()) => 0,
		Err(_) => 1,
	};
	match mode.as_str() {
		login::web_login::HELPER_FLAG => {
			let ipc_name = args.next().unwrap_or_default();
			Some(code(login::web_login::run_helper(&ipc_name)))
		}
		login::web_login::CAPTCHA_HELPER_FLAG => {
			let ipc_name = args.next().unwrap_or_default();
			let sitekey = args.next().unwrap_or_default();
			let rqdata = args.next().filter(|value| !value.is_empty());
			let invisible = args.next().as_deref() == Some("--invisible");
			Some(code(login::web_login::run_captcha_helper(
				&ipc_name,
				&sitekey,
				rqdata.as_deref(),
				invisible,
			)))
		}
		_ => None,
	}
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
