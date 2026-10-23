use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::traits::tokio::Listener as _;
use interprocess::local_socket::{
	GenericNamespaced, ListenerOptions, Name, Stream, ToNsName, tokio as async_socket,
};
use parking_lot::Mutex;
use protocol::Token;
use rand::RngCore;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::WindowBuilder;
use tokio::io::AsyncReadExt;
use tokio::time::{interval, timeout};
use wry::http::Request;
use wry::{WebContext, WebViewBuilder};
use zeroize::Zeroizing;

use crate::error::{LoginError, Result};
use crate::token_login::{clean_token, plausible};

pub const HELPER_FLAG: &str = "--login-helper";

const LOGIN_URL: &str = "https://discord.com/login";
const LOGIN_HOST: &str = "discord.com";
const WINDOW_TITLE: &str = "Sign in to Discord";
const IPC_PREFIX: &str = "oxidcord-login-";
const MAX_TOKEN_BYTES: usize = 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const EXIT_POLL: Duration = Duration::from_millis(250);
const LATE_TOKEN_GRACE: Duration = Duration::from_millis(750);
const REAP_ATTEMPTS: u32 = 30;
const REMOVE_ATTEMPTS: u32 = 30;
const RETRY_DELAY: Duration = Duration::from_millis(100);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const TOKEN_PROBE: &str = r#"
(() => {
	if (window.top !== window || location.origin !== 'https://discord.com') {
		return;
	}
	const direct = window.localStorage;
	let probe = null;
	const storage = () => {
		if (direct) {
			return direct;
		}
		if (!probe && document.body) {
			const frame = document.createElement('iframe');
			frame.style.display = 'none';
			document.body.appendChild(frame);
			probe = frame.contentWindow.localStorage;
		}
		return probe;
	};
	const timer = setInterval(() => {
		try {
			const area = storage();
			const stored = area && area.getItem('token');
			if (!stored) {
				return;
			}
			const token = stored.trim().replace(/^"+|"+$/g, '');
			if (token) {
				clearInterval(timer);
				window.ipc.postMessage(token);
			}
		} catch (error) {}
	}, 1000);
})();
"#;

pub struct WebLogin;

impl WebLogin {
	pub fn start() -> Result<WebLoginSession> {
		Self::start_with(&std::env::current_exe()?)
	}

	pub fn start_with(executable: &Path) -> Result<WebLoginSession> {
		let ipc_name = random_ipc_name();
		let listener = ListenerOptions::new()
			.name(socket_name(&ipc_name)?)
			.create_tokio()?;
		let child = spawn_helper(executable, &ipc_name)?;
		Ok(WebLoginSession {
			listener,
			child: Mutex::new(child),
			data_dir: data_dir(&ipc_name),
		})
	}
}

pub struct WebLoginSession {
	listener: async_socket::Listener,
	child: Mutex<Child>,
	data_dir: PathBuf,
}

impl WebLoginSession {
	pub async fn wait(&self) -> Result<Token> {
		let accept = self.listener.accept();
		tokio::pin!(accept);
		let mut exit_poll = interval(EXIT_POLL);
		let stream = loop {
			tokio::select! {
				accepted = &mut accept => break accepted?,
				_ = exit_poll.tick() => {
					let status = self.child.lock().try_wait()?;
					if let Some(status) = status {
						return match timeout(LATE_TOKEN_GRACE, &mut accept).await {
							Ok(accepted) => self.receive(accepted?).await,
							Err(_) if status.success() => Err(LoginError::Cancelled),
							Err(_) => Err(LoginError::Helper(format!("helper exited with {status}"))),
						};
					}
				}
			}
		};
		self.receive(stream).await
	}

	pub fn abort(&self) {
		self.shutdown();
	}

	async fn receive(&self, stream: async_socket::Stream) -> Result<Token> {
		let token = read_token(stream).await;
		self.reap().await;
		token
	}

	async fn reap(&self) {
		for _ in 0..REAP_ATTEMPTS {
			if !matches!(self.child.lock().try_wait(), Ok(None)) {
				return;
			}
			tokio::time::sleep(RETRY_DELAY).await;
		}
	}

	fn shutdown(&self) {
		let mut child = self.child.lock();
		if matches!(child.try_wait(), Ok(None)) {
			let _ = child.kill();
			let _ = child.wait();
		}
		let data_dir = self.data_dir.clone();
		thread::spawn(move || remove_data_dir(&data_dir));
	}
}

impl Drop for WebLoginSession {
	fn drop(&mut self) {
		self.shutdown();
	}
}

async fn read_token(stream: async_socket::Stream) -> Result<Token> {
	let mut received = Zeroizing::new(Vec::with_capacity(MAX_TOKEN_BYTES + 1));
	let mut limited = stream.take(MAX_TOKEN_BYTES as u64 + 1);
	timeout(READ_TIMEOUT, limited.read_to_end(&mut received))
		.await
		.map_err(|_| LoginError::Helper("helper stalled while sending the token".to_owned()))??;
	let text = std::str::from_utf8(&received)
		.map_err(|_| LoginError::Helper("helper sent a malformed token".to_owned()))?;
	let cleaned = clean_token(text);
	if !plausible(cleaned) {
		return Err(LoginError::Helper("helper sent a malformed token".to_owned()));
	}
	Ok(Token::new(cleaned))
}

fn random_ipc_name() -> String {
	let mut random = [0u8; 16];
	rand::rng().fill_bytes(&mut random);
	format!("{IPC_PREFIX}{}", hex::encode(random))
}

fn socket_name(ipc_name: &str) -> io::Result<Name<'_>> {
	ipc_name.to_ns_name::<GenericNamespaced>()
}

fn data_dir(ipc_name: &str) -> PathBuf {
	std::env::temp_dir().join(ipc_name)
}

fn valid_ipc_name(ipc_name: &str) -> bool {
	ipc_name.starts_with(IPC_PREFIX)
		&& ipc_name.len() <= 64
		&& ipc_name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn remove_data_dir(path: &Path) {
	for _ in 0..REMOVE_ATTEMPTS {
		match fs::remove_dir_all(path) {
			Ok(()) => return,
			Err(error) if error.kind() == io::ErrorKind::NotFound => return,
			Err(_) => thread::sleep(RETRY_DELAY),
		}
	}
	tracing::warn!("could not remove the login webview data directory");
}

pub fn spawn_helper(current_exe: &Path, ipc_name: &str) -> io::Result<Child> {
	let mut command = Command::new(current_exe);
	command
		.arg(HELPER_FLAG)
		.arg(ipc_name)
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null());
	#[cfg(windows)]
	std::os::windows::process::CommandExt::creation_flags(&mut command, CREATE_NO_WINDOW);
	command.spawn()
}

pub fn helper_ipc_name(args: impl IntoIterator<Item = String>) -> Option<String> {
	let mut args = args.into_iter();
	args.by_ref().find(|arg| arg == HELPER_FLAG)?;
	args.next()
}

enum HelperEvent {
	Token(Zeroizing<String>),
}

pub fn run_helper(ipc_name: &str) -> Result<()> {
	if !valid_ipc_name(ipc_name) {
		return Err(LoginError::Helper("invalid ipc name".to_owned()));
	}
	let data_dir = data_dir(ipc_name);
	fs::create_dir_all(&data_dir)?;
	let outcome = show_login_window(ipc_name, &data_dir);
	remove_data_dir(&data_dir);
	outcome
}

fn helper_failure(error: impl std::fmt::Display) -> LoginError {
	LoginError::Helper(error.to_string())
}

fn show_login_window(ipc_name: &str, data_dir: &Path) -> Result<()> {
	let mut event_loop = EventLoopBuilder::<HelperEvent>::with_user_event().build();
	let proxy = event_loop.create_proxy();
	let window = WindowBuilder::new()
		.with_title(WINDOW_TITLE)
		.with_inner_size(LogicalSize::new(520.0, 760.0))
		.build(&event_loop)
		.map_err(helper_failure)?;
	let mut context = WebContext::new(Some(data_dir.to_path_buf()));
	let webview = WebViewBuilder::new_with_web_context(&mut context)
		.with_initialization_script(TOKEN_PROBE)
		.with_ipc_handler(move |request| forward_token(&proxy, &request))
		.with_url(LOGIN_URL)
		.build(&window)
		.map_err(helper_failure)?;
	let mut outcome = Ok(());
	event_loop.run_return(|event, _, control_flow| {
		*control_flow = ControlFlow::Wait;
		match event {
			Event::UserEvent(HelperEvent::Token(token)) => {
				outcome = deliver(ipc_name, &token);
				*control_flow = ControlFlow::Exit;
			}
			Event::WindowEvent {
				event: WindowEvent::CloseRequested,
				..
			} => *control_flow = ControlFlow::Exit,
			_ => {}
		}
	});
	let _ = webview.clear_all_browsing_data();
	outcome
}

fn forward_token(proxy: &EventLoopProxy<HelperEvent>, request: &Request<String>) {
	if request.uri().host() != Some(LOGIN_HOST) {
		return;
	}
	let token = Zeroizing::new(clean_token(request.body()).to_owned());
	if plausible(&token) {
		let _ = proxy.send_event(HelperEvent::Token(token));
	}
}

fn deliver(ipc_name: &str, token: &str) -> Result<()> {
	let mut stream = Stream::connect(socket_name(ipc_name)?)?;
	stream.write_all(token.as_bytes())?;
	stream.flush()?;
	Ok(())
}
