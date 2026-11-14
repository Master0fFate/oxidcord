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
use rest::{Request as RestRequest, RestClient, RestConfig};
use serde::{Deserialize, Serialize};
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
use crate::token_login::plausible;

pub const HELPER_FLAG: &str = "--login-helper";
pub const CAPTCHA_HELPER_FLAG: &str = "--captcha-helper";

const LOGIN_URL: &str = "https://discord.com/login";
const LOGIN_HOST: &str = "discord.com";
const WINDOW_TITLE: &str = "Sign in to Discord";
const CAPTCHA_TITLE: &str = "Discord captcha";
const IPC_PREFIX: &str = "oxidcord-login-";
const MAX_HANDOFF_BYTES: usize = 2048;
const MAX_HANDOFF_TOKEN_BYTES: usize = 1024;
const MAX_HANDOFF_KEY_BYTES: usize = 128;
const MAX_CAPTCHA_BYTES: usize = 8192;
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
	let originalStorage;
	try {
		originalStorage = window.localStorage;
	} catch (error) {}
	let fallbackStorage;
	const initialize = () => {
		const control = document.createElement('div');
		control.style.cssText = 'position:fixed;bottom:8px;right:8px;z-index:2147483647;background:#202225;color:white;padding:8px;border-radius:6px;font:14px sans-serif;max-width:240px';
		const status = document.createElement('div');
		status.textContent = 'Finish signing in, then select Continue.';
		status.setAttribute('role', 'status');
		const button = document.createElement('button');
		button.textContent = 'Continue';
		control.append(status, button);
		document.body.appendChild(control);
		button.addEventListener('click', async () => {
			if (button.disabled) return;
			if (!location.pathname.startsWith('/channels/')) {
				status.textContent = 'Finish signing in to Discord first.';
				return;
			}
			let token;
			try {
				let area = originalStorage;
				if (!area) {
					if (!fallbackStorage) {
						const frame = document.createElement('iframe');
						frame.style.display = 'none';
						document.body.appendChild(frame);
						fallbackStorage = frame.contentWindow.localStorage;
					}
					area = fallbackStorage;
				}
				const stored = area.getItem('token');
				token = stored;
				if (stored !== null) {
					try { token = JSON.parse(stored); } catch (error) {}
				}
			} catch (error) {}
			if (typeof token !== 'string' || !token.trim()) {
				status.textContent = 'Sign in to Discord, then select Continue.';
				return;
			}
			button.disabled = true;
			let submitted = false;
			status.textContent = 'Starting login handoff...';
			try {
				const key = crypto.randomUUID();
				const response = await fetch('https://discord.com/api/v9/auth/handoff', {
					method: 'POST',
					headers: { 'Authorization': token, 'Content-Type': 'application/json' },
					credentials: 'same-origin',
					redirect: 'error',
					body: JSON.stringify({ key })
				});
				if (!response.ok) {
					status.textContent = `Login handoff failed (HTTP ${response.status}). Try again.`;
					return;
				}
				const data = await response.json();
				if (typeof data?.handoff_token !== 'string' || !/^[\x21-\x7e]{1,1024}$/.test(data.handoff_token)) {
					status.textContent = 'Invalid handoff response. Try again.';
					return;
				}
				const message = JSON.stringify({ key, handoff_token: data.handoff_token });
				if (message.length > 2048) {
					status.textContent = 'Invalid handoff response. Try again.';
					return;
				}
				window.ipc.postMessage(message);
				submitted = true;
				status.textContent = 'Finishing login...';
				return;
			} catch (error) {
				status.textContent = 'Login handoff failed. Check your connection or try again.';
			} finally {
				button.disabled = submitted;
			}
		});
	};
	if (document.readyState === 'loading') {
		document.addEventListener('DOMContentLoaded', initialize, { once: true });
	} else {
		initialize();
	}
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
		tracing::info!("web login helper started");
		Ok(WebLoginSession {
			session: HelperSession {
				listener,
				child: Mutex::new(child),
				data_dir: data_dir(&ipc_name),
			},
		})
	}
}

pub struct CaptchaLogin;

impl CaptchaLogin {
	pub fn start(sitekey: &str, rqdata: Option<&str>, invisible: bool) -> Result<CaptchaSession> {
		if sitekey.is_empty() {
			return Err(LoginError::Helper("captcha site key missing".to_owned()));
		}
		let ipc_name = random_ipc_name();
		let listener = ListenerOptions::new()
			.name(socket_name(&ipc_name)?)
			.create_tokio()?;
		let child = spawn_captcha_helper(&std::env::current_exe()?, &ipc_name, sitekey, rqdata, invisible)?;
		Ok(CaptchaSession {
			session: HelperSession {
				listener,
				child: Mutex::new(child),
				data_dir: data_dir(&ipc_name),
			},
		})
	}
}

pub struct HelperSession {
	listener: async_socket::Listener,
	child: Mutex<Child>,
	data_dir: PathBuf,
}

impl HelperSession {
	async fn accept(&self) -> Result<async_socket::Stream> {
		let accept = self.listener.accept();
		tokio::pin!(accept);
		let mut exit_poll = interval(EXIT_POLL);
		loop {
			tokio::select! {
				accepted = &mut accept => return Ok(accepted?),
				_ = exit_poll.tick() => {
					let status = self.child.lock().try_wait()?;
					if let Some(status) = status {
						return match timeout(LATE_TOKEN_GRACE, &mut accept).await {
							Ok(accepted) => Ok(accepted?),
							Err(_) if status.success() => Err(LoginError::Cancelled),
							Err(_) => Err(LoginError::Helper(format!("helper exited with {status}"))),
						};
					}
				}
			}
		}
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

pub struct WebLoginSession {
	session: HelperSession,
}

impl WebLoginSession {
	pub async fn wait(&self, rest_base: &url::Url) -> Result<Token> {
		let stream = self.session.accept().await?;
		let handoff = read_handoff(stream).await;
		self.session.reap().await;
		let handoff = handoff?;
		let client = RestClient::new(RestConfig {
			base_url: rest_base.clone(),
			..RestConfig::default()
		});
		tracing::info!("web login handoff exchange started");
		let response: HandoffResponse = client
			.send(RestRequest::post("/auth/handoff/exchange").json(&handoff))
			.await
			.map_err(|error| {
				tracing::info!(
					status = ?error.status_code(),
					api_code = ?error.api().map(|api| api.code),
					"web login handoff exchange failed"
				);
				match error {
					rest::RestError::Decode(_) => {
						LoginError::Protocol("malformed handoff response".to_owned())
					}
					other => other.into(),
				}
			})?;
		if !plausible(response.token.as_str()) {
			return Err(LoginError::Protocol("malformed handoff response".to_owned()));
		}
		tracing::info!("web login handoff exchanged");
		Ok(response.token)
	}

	pub fn abort(&self) {
		self.session.shutdown();
	}
}

impl Drop for WebLoginSession {
	fn drop(&mut self) {
		self.session.shutdown();
	}
}

pub struct CaptchaSession {
	session: HelperSession,
}

impl CaptchaSession {
	pub async fn wait(&self) -> Result<String> {
		let stream = self.session.accept().await?;
		let token = read_captcha(stream).await;
		self.session.reap().await;
		token
	}

	pub fn abort(&self) {
		self.session.shutdown();
	}
}

impl Drop for CaptchaSession {
	fn drop(&mut self) {
		self.session.shutdown();
	}
}

#[derive(Deserialize, Serialize)]
struct Handoff {
	key: String,
	handoff_token: Token,
}

#[derive(Deserialize)]
struct HandoffResponse {
	token: Token,
}

fn parse_handoff(message: &[u8]) -> Result<Handoff> {
	let malformed = || LoginError::Helper("helper sent a malformed handoff".to_owned());
	if message.len() > MAX_HANDOFF_BYTES {
		return Err(malformed());
	}
	let handoff: Handoff = serde_json::from_slice(message).map_err(|_| malformed())?;
	let valid = |value: &str, max: usize| {
		!value.is_empty() && value.len() <= max && value.bytes().all(|byte| byte.is_ascii_graphic())
	};
	if !valid(&handoff.key, MAX_HANDOFF_KEY_BYTES)
		|| !valid(handoff.handoff_token.as_str(), MAX_HANDOFF_TOKEN_BYTES)
	{
		return Err(malformed());
	}
	Ok(handoff)
}

async fn read_handoff(stream: async_socket::Stream) -> Result<Handoff> {
	let mut received = Zeroizing::new(Vec::with_capacity(MAX_HANDOFF_BYTES + 1));
	let mut limited = stream.take(MAX_HANDOFF_BYTES as u64 + 1);
	timeout(READ_TIMEOUT, limited.read_to_end(&mut received))
		.await
		.map_err(|_| LoginError::Helper("helper stalled while sending the handoff".to_owned()))??;
	let handoff = parse_handoff(&received)?;
	tracing::info!("web login handoff received");
	Ok(handoff)
}

async fn read_captcha(stream: async_socket::Stream) -> Result<String> {
	let mut received = Zeroizing::new(Vec::with_capacity(MAX_CAPTCHA_BYTES + 10));
	let mut limited = stream.take(MAX_CAPTCHA_BYTES as u64 + 10);
	timeout(READ_TIMEOUT, limited.read_to_end(&mut received))
		.await
		.map_err(|_| LoginError::Helper("helper stalled while sending the captcha".to_owned()))??;
	let text = std::str::from_utf8(&received)
		.map_err(|_| LoginError::Helper("helper sent a malformed captcha".to_owned()))?;
	if let Some(reason) = captcha_failure(text) {
		return Err(LoginError::Helper(reason.to_owned()));
	}
	let token = text.strip_prefix("solution:").unwrap_or_default().trim();
	if token.is_empty() || token.len() > MAX_CAPTCHA_BYTES {
		return Err(LoginError::Helper("helper sent a malformed captcha".to_owned()));
	}
	Ok(token.to_owned())
}

fn captcha_failure(message: &str) -> Option<&'static str> {
	match message {
		"failure:load" => Some("captcha failed to load"),
		"failure:timeout" => Some("captcha loading timed out"),
		"failure:render" => Some("captcha failed to render"),
		"failure:challenge" => Some("captcha challenge failed"),
		_ => None,
	}
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

fn spawn_captcha_helper(
	executable: &Path,
	ipc_name: &str,
	sitekey: &str,
	rqdata: Option<&str>,
	invisible: bool,
) -> io::Result<Child> {
	let mut command = Command::new(executable);
	command
		.arg(CAPTCHA_HELPER_FLAG)
		.arg(ipc_name)
		.arg(sitekey)
		.arg(rqdata.unwrap_or_default());
	if invisible {
		command.arg("--invisible");
	}
	command
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
	run_helper_window(ipc_name, HelperMode::Web)
}

pub fn run_captcha_helper(
	ipc_name: &str,
	sitekey: &str,
	rqdata: Option<&str>,
	invisible: bool,
) -> Result<()> {
	if sitekey.is_empty() {
		return Err(LoginError::Helper("captcha site key missing".to_owned()));
	}
	run_helper_window(
		ipc_name,
		HelperMode::Captcha {
			sitekey,
			rqdata,
			invisible,
		},
	)
}

enum HelperMode<'a> {
	Web,
	Captcha {
		sitekey: &'a str,
		rqdata: Option<&'a str>,
		invisible: bool,
	},
}

fn run_helper_window(ipc_name: &str, mode: HelperMode<'_>) -> Result<()> {
	if !valid_ipc_name(ipc_name) {
		return Err(LoginError::Helper("invalid ipc name".to_owned()));
	}
	let data_dir = data_dir(ipc_name);
	fs::create_dir_all(&data_dir)?;
	let outcome = show_login_window(ipc_name, &data_dir, mode);
	remove_data_dir(&data_dir);
	outcome
}

fn helper_failure(error: impl std::fmt::Display) -> LoginError {
	LoginError::Helper(error.to_string())
}

fn show_login_window(ipc_name: &str, data_dir: &Path, mode: HelperMode<'_>) -> Result<()> {
	let (script, title, captcha) = match mode {
		HelperMode::Web => (TOKEN_PROBE.to_owned(), WINDOW_TITLE, false),
		HelperMode::Captcha {
			sitekey,
			rqdata,
			invisible,
		} => (captcha_probe(sitekey, rqdata, invisible), CAPTCHA_TITLE, true),
	};
	let mut event_loop = EventLoopBuilder::<HelperEvent>::with_user_event().build();
	let proxy = event_loop.create_proxy();
	let window = WindowBuilder::new()
		.with_title(title)
		.with_inner_size(LogicalSize::new(520.0, 760.0))
		.build(&event_loop)
		.map_err(helper_failure)?;
	let mut context = WebContext::new(Some(data_dir.to_path_buf()));
	let webview = WebViewBuilder::new_with_web_context(&mut context)
		.with_initialization_script(&script)
		.with_ipc_handler(move |request| {
			if captcha {
				forward_captcha(&proxy, &request);
			} else {
				forward_token(&proxy, &request);
			}
		})
		.with_url(LOGIN_URL)
		.build(&window)
		.map_err(helper_failure)?;
	let mut outcome = Ok(());
	let mut pending: Option<Zeroizing<String>> = None;
	event_loop.run_return(|event, _, control_flow| {
		*control_flow = ControlFlow::Wait;
		match event {
			Event::UserEvent(HelperEvent::Token(token)) => {
				pending = Some(token);
				*control_flow = ControlFlow::Exit;
			}
			Event::WindowEvent {
				event: WindowEvent::CloseRequested,
				..
			} => *control_flow = ControlFlow::Exit,
			_ => {}
		}
	});
	let _ = webview.load_url("about:blank");
	drop(webview);
	drop(window);
	if let Some(token) = pending {
		outcome = deliver(ipc_name, &token);
	}
	outcome
}

fn forward_token(proxy: &EventLoopProxy<HelperEvent>, request: &Request<String>) {
	if request.uri().host() != Some(LOGIN_HOST) {
		return;
	}
	if parse_handoff(request.body().as_bytes()).is_ok() {
		let handoff = Zeroizing::new(request.body().to_owned());
		let _ = proxy.send_event(HelperEvent::Token(handoff));
	}
}

fn forward_captcha(proxy: &EventLoopProxy<HelperEvent>, request: &Request<String>) {
	if request.uri().host() != Some(LOGIN_HOST) {
		return;
	}
	let message = request.body();
	let forwarded = if let Some(token) = message.strip_prefix("solution:") {
		if token.trim().is_empty() || token.len() > MAX_CAPTCHA_BYTES {
			"failure:challenge"
		} else {
			message
		}
	} else if captcha_failure(message).is_some() {
		message
	} else {
		return;
	};
	let _ = proxy.send_event(HelperEvent::Token(Zeroizing::new(forwarded.to_owned())));
}

fn captcha_probe(sitekey: &str, rqdata: Option<&str>, invisible: bool) -> String {
	let sitekey = serde_json::to_string(sitekey).unwrap_or_else(|_| "\"\"".to_owned());
	let rqdata = rqdata
		.map(|value| serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()))
		.unwrap_or_else(|| "null".to_owned());
	format!(
		r#"
(() => {{
	if (window.top !== window || location.origin !== 'https://discord.com') {{
		return;
	}}
	const sitekey = {sitekey};
	const rqdata = {rqdata};
	const invisible = {invisible};
	const initialize = () => {{
		const box = document.createElement('div');
		box.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#0b0b0b;color:white;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:16px;font:16px sans-serif';
		const status = document.createElement('p');
		status.textContent = 'Loading captcha...';
		status.setAttribute('role', 'status');
		const widget = document.createElement('div');
		const retry = document.createElement('button');
		retry.textContent = invisible ? 'Start captcha' : 'Try again';
		retry.hidden = true;
		const disclosure = document.createElement('p');
		disclosure.append('This site is protected by hCaptcha and its ');
		for (const [label, url] of [['Privacy Policy', 'https://www.hcaptcha.com/privacy'], ['Terms of Service', 'https://www.hcaptcha.com/terms']]) {{
			if (label === 'Terms of Service') disclosure.append(' and ');
			const link = document.createElement('a');
			link.href = url;
			link.textContent = label;
			link.style.color = 'white';
			disclosure.append(link);
		}}
		disclosure.append(' apply.');
		box.append(status, widget, retry, disclosure);
		document.body.appendChild(box);
		let finished = false;
		let ready = false;
		let widgetId;
		const fail = (reason, message) => {{
			if (finished) return;
			finished = true;
			clearTimeout(timer);
			status.textContent = message;
			window.ipc.postMessage('failure:' + reason);
		}};
		const timer = setTimeout(() => fail('timeout', 'Captcha loading timed out.'), 15000);
		window.oxidcordCaptchaReady = () => {{
			if (finished || ready) return;
			ready = true;
			clearTimeout(timer);
			const expired = () => {{
				if (finished) return;
				status.textContent = 'Captcha closed or expired. Try again.';
				retry.textContent = 'Try again';
				retry.hidden = false;
			}};
			const options = {{
				sitekey,
				callback: (token) => {{
					if (finished) return;
					finished = true;
					window.ipc.postMessage('solution:' + token);
				}},
				'error-callback': () => fail('challenge', 'Captcha challenge failed.'),
				'expired-callback': expired,
				'chalexpired-callback': expired,
				'close-callback': expired
			}};
			if (invisible) options.size = 'invisible';
			try {{
				widgetId = window.hcaptcha.render(widget, options);
				if (!invisible && rqdata) {{
					if (typeof window.hcaptcha.setData !== 'function') {{
						fail('render', 'Captcha could not be configured.');
						return;
					}}
					window.hcaptcha.setData(widgetId, {{ rqdata }});
				}}
				if (!finished) {{
					status.textContent = invisible ? 'Start the captcha to continue.' : 'Complete the captcha to continue.';
					retry.hidden = !invisible;
				}}
			}} catch (error) {{
				fail('render', 'Captcha failed to render.');
			}}
		}};
		let started = false;
		retry.addEventListener('click', () => {{
			try {{
				if (started || !invisible) {{
					window.hcaptcha.reset(widgetId);
					if (!invisible && rqdata) window.hcaptcha.setData(widgetId, {{ rqdata }});
				}}
				retry.hidden = true;
				status.textContent = 'Complete the captcha to continue.';
				if (invisible) {{
					started = true;
					window.hcaptcha.execute(widgetId, rqdata ? {{ rqdata }} : {{}});
				}}
			}} catch (error) {{
				fail('render', 'Captcha failed to start.');
			}}
		}});
		const existing = document.querySelector('script[src^="https://js.hcaptcha.com/1/api.js"]');
		const useExisting = () => {{
			if (window.hcaptcha && typeof window.hcaptcha.render === 'function') {{
				window.oxidcordCaptchaReady();
			}}
		}};
		if (window.hcaptcha && typeof window.hcaptcha.render === 'function') {{
			useExisting();
		}} else if (existing) {{
			existing.addEventListener('load', useExisting, {{ once: true }});
			existing.addEventListener('error', () => fail('load', 'Captcha failed to load.'), {{ once: true }});
		}} else {{
			const script = document.createElement('script');
			script.src = 'https://js.hcaptcha.com/1/api.js?onload=oxidcordCaptchaReady&render=explicit';
			script.onerror = () => fail('load', 'Captcha failed to load.');
			document.head.appendChild(script);
		}}
	}};
	if (document.readyState === 'loading') {{
		document.addEventListener('DOMContentLoaded', initialize, {{ once: true }});
	}} else {{
		initialize();
	}}
}})();
"#
	)
}

fn deliver(ipc_name: &str, token: &str) -> Result<()> {
	let mut stream = Stream::connect(socket_name(ipc_name)?)?;
	stream.write_all(token.as_bytes())?;
	stream.flush()?;
	Ok(())
}
