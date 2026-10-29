#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
	pub x: i32,
	pub y: i32,
	pub width: i32,
	pub height: i32,
}

impl Rect {
	pub fn is_empty(self) -> bool {
		self.width <= 0 || self.height <= 0
	}
}

pub use host::Host;

#[cfg(windows)]
mod host {
	use std::borrow::Cow;
	use std::cell::{Cell, RefCell};
	use std::collections::HashMap;
	use std::num::NonZeroIsize;
	use std::path::PathBuf;
	use std::sync::mpsc::{Receiver, Sender, channel};
	use std::thread;

	use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
	use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
	use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, ClientToScreen, GetStockObject, HBRUSH};
	use windows::Win32::System::LibraryLoader::GetModuleHandleW;
	use windows::Win32::UI::WindowsAndMessaging::{
		CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GW_HWNDPREV, GetForegroundWindow,
		GetMessageW, GetWindow, HWND_MESSAGE, HWND_TOP, IsIconic, IsWindowVisible, KillTimer, MA_NOACTIVATE,
		MSG, PostMessageW, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
		SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetTimer, SetWindowPos, ShowWindow, TranslateMessage,
		WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_MOUSEACTIVATE, WM_TIMER, WNDCLASSW,
		WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
	};
	use windows::core::{PCWSTR, w};
	use wry::dpi::{PhysicalPosition, PhysicalSize};
	use wry::http::header::CONTENT_TYPE;
	use wry::http::{Request, Response};
	use wry::{
		NewWindowResponse, PermissionResponse, RGBA, Rect as Bounds, WebContext, WebView, WebViewBuilder,
		WebViewBuilderExtWindows, WebViewId,
	};

	use super::Rect;

	const HOST_CLASS: PCWSTR = w!("OxidcordEmbedHost");
	const FRAME_CLASS: PCWSTR = w!("OxidcordEmbedFrame");
	const PROTOCOL: &str = "oxidcord";
	const FRAME_URL: &str = "oxidcord://localhost/frame";
	const FRAME_ORIGIN: &str = "https://oxidcord.localhost/";
	const WAKE: u32 = WM_APP + 1;
	const TICK: usize = 1;
	const TICK_MS: u32 = 16;
	const BACKGROUND: RGBA = (0, 0, 0, 255);

	enum Command {
		Open { id: u64, document: String },
		Place { id: u64, rect: Rect, clip: Rect },
		Hide { id: u64 },
		Close { id: u64 },
	}

	pub struct Host {
		commands: Sender<Command>,
		wake: isize,
	}

	struct Parent(HWND);

	impl HasWindowHandle for Parent {
		fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
			let handle = NonZeroIsize::new(self.0.0 as isize).ok_or(HandleError::Unavailable)?;
			Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(handle))) })
		}
	}

	struct Frame {
		popup: HWND,
		webview: WebView,
		rect: Rect,
		clip: Rect,
		shown: bool,
		placed: Option<(Rect, POINT)>,
	}

	struct State {
		owner: HWND,
		message_window: HWND,
		commands: Receiver<Command>,
		context: WebContext,
		frames: HashMap<u64, Frame>,
		ticking: bool,
	}

	thread_local! {
		static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
		static PENDING: Cell<bool> = const { Cell::new(false) };
	}

	impl Host {
		pub fn start(owner: isize, data_dir: PathBuf) -> Option<Self> {
			let (commands, receiver) = channel();
			let (ready, started) = channel();
			thread::Builder::new()
				.name("embed-host".to_owned())
				.spawn(move || run(owner, data_dir, receiver, ready))
				.ok()?;
			let wake = started.recv().ok()??;
			Some(Self { commands, wake })
		}

		fn send(&self, command: Command) {
			if self.commands.send(command).is_ok() {
				let _ = unsafe { PostMessageW(Some(HWND(self.wake as *mut _)), WAKE, WPARAM(0), LPARAM(0)) };
			}
		}

		pub fn open(&self, id: u64, document: String) {
			self.send(Command::Open { id, document });
		}

		pub fn place(&self, id: u64, rect: Rect, clip: Rect) {
			self.send(Command::Place { id, rect, clip });
		}

		pub fn hide(&self, id: u64) {
			self.send(Command::Hide { id });
		}

		pub fn close(&self, id: u64) {
			self.send(Command::Close { id });
		}
	}

	impl Drop for Host {
		fn drop(&mut self) {
			let _ = unsafe { PostMessageW(Some(HWND(self.wake as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
		}
	}

	fn register(
		class: PCWSTR,
		procedure: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
	) -> bool {
		let instance = unsafe { GetModuleHandleW(None) }.ok();
		let class = WNDCLASSW {
			lpfnWndProc: Some(procedure),
			hInstance: instance.map(Into::into).unwrap_or_default(),
			lpszClassName: class,
			hbrBackground: HBRUSH(unsafe { GetStockObject(BLACK_BRUSH) }.0),
			..WNDCLASSW::default()
		};
		unsafe { RegisterClassW(&class) != 0 }
	}

	fn run(owner: isize, data_dir: PathBuf, commands: Receiver<Command>, ready: Sender<Option<isize>>) {
		if !register(HOST_CLASS, host_procedure) || !register(FRAME_CLASS, frame_procedure) {
			let _ = ready.send(None);
			return;
		}
		let created = unsafe {
			CreateWindowExW(
				WINDOW_EX_STYLE::default(),
				HOST_CLASS,
				PCWSTR::null(),
				Default::default(),
				0,
				0,
				0,
				0,
				Some(HWND_MESSAGE),
				None,
				None,
				None,
			)
		};
		let Ok(message_window) = created else {
			let _ = ready.send(None);
			return;
		};
		STATE.with(|state| {
			*state.borrow_mut() = Some(State {
				owner: HWND(owner as *mut _),
				message_window,
				commands,
				context: WebContext::new(Some(data_dir)),
				frames: HashMap::new(),
				ticking: false,
			});
		});
		let _ = ready.send(Some(message_window.0 as isize));
		let mut message = MSG::default();
		while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
			unsafe {
				let _ = TranslateMessage(&message);
				DispatchMessageW(&message);
			}
		}
		STATE.with(|state| state.borrow_mut().take());
	}

	unsafe extern "system" fn frame_procedure(
		window: HWND,
		message: u32,
		wparam: WPARAM,
		lparam: LPARAM,
	) -> LRESULT {
		if message == WM_MOUSEACTIVATE {
			return LRESULT(MA_NOACTIVATE as isize);
		}
		unsafe { DefWindowProcW(window, message, wparam, lparam) }
	}

	unsafe extern "system" fn host_procedure(
		window: HWND,
		message: u32,
		wparam: WPARAM,
		lparam: LPARAM,
	) -> LRESULT {
		match message {
			WAKE => drain(),
			WM_TIMER => with_state(State::follow_owner),
			WM_CLOSE => {
				with_state(State::close_all);
				let _ = unsafe { DestroyWindow(window) };
			}
			WM_DESTROY => unsafe { PostQuitMessage(0) },
			_ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
		}
		LRESULT(0)
	}

	fn with_state(action: impl FnOnce(&mut State)) {
		STATE.with(|state| match state.try_borrow_mut() {
			Ok(mut state) => {
				if let Some(state) = state.as_mut() {
					action(state);
				}
			}
			Err(_) => PENDING.set(true),
		});
	}

	fn drain() {
		loop {
			PENDING.set(false);
			with_state(|state| {
				while let Ok(command) = state.commands.try_recv() {
					state.apply(command);
				}
			});
			if !PENDING.get() {
				return;
			}
		}
	}

	fn document_response(document: &str) -> Response<Cow<'static, [u8]>> {
		let policy = format!(
			"default-src 'none'; style-src 'unsafe-inline'; frame-src {}",
			media::providers::FRAME_ORIGINS.join(" ")
		);
		Response::builder()
			.header(CONTENT_TYPE, "text/html; charset=utf-8")
			.header("Content-Security-Policy", policy)
			.header("Referrer-Policy", "strict-origin")
			.header("X-Content-Type-Options", "nosniff")
			.body(Cow::Owned(document.as_bytes().to_vec()))
			.unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
	}

	fn bounds(rect: Rect, clip: Rect) -> Bounds {
		Bounds {
			position: PhysicalPosition::new(rect.x - clip.x, rect.y - clip.y).into(),
			size: PhysicalSize::new(rect.width.max(1) as u32, rect.height.max(1) as u32).into(),
		}
	}

	impl State {
		fn apply(&mut self, command: Command) {
			match command {
				Command::Open { id, document } => self.open(id, document),
				Command::Place { id, rect, clip } => {
					if let Some(frame) = self.frames.get_mut(&id) {
						frame.rect = rect;
						frame.clip = clip;
						frame.shown = !clip.is_empty();
					}
					self.follow_owner();
				}
				Command::Hide { id } => {
					if let Some(frame) = self.frames.get_mut(&id) {
						frame.shown = false;
					}
					self.follow_owner();
				}
				Command::Close { id } => {
					if let Some(frame) = self.frames.remove(&id) {
						frame.destroy();
					}
					self.update_timer();
				}
			}
		}

		fn open(&mut self, id: u64, document: String) {
			if self.frames.contains_key(&id) {
				return;
			}
			let instance = unsafe { GetModuleHandleW(None) }.ok();
			let created = unsafe {
				CreateWindowExW(
					WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
					FRAME_CLASS,
					PCWSTR::null(),
					WS_POPUP | WS_CLIPCHILDREN,
					0,
					0,
					1,
					1,
					None,
					None,
					instance.map(Into::into),
					None,
				)
			};
			let Ok(popup) = created else {
				return;
			};
			let parent = Parent(popup);
			let built = WebViewBuilder::new_with_web_context(&mut self.context)
				.with_custom_protocol(PROTOCOL.to_owned(), move |_: WebViewId, _: Request<Vec<u8>>| {
					document_response(&document)
				})
				.with_https_scheme(true)
				.with_url(FRAME_URL)
				.with_bounds(bounds(Rect::default(), Rect::default()))
				.with_background_color(BACKGROUND)
				.with_autoplay(true)
				.with_devtools(false)
				.with_clipboard(false)
				.with_hotkeys_zoom(false)
				.with_focused(false)
				.with_browser_accelerator_keys(false)
				.with_default_context_menus(false)
				.with_navigation_handler(|url| url.starts_with(FRAME_ORIGIN))
				.with_new_window_req_handler(|url, _| {
					if url.starts_with("https://")
						&& let Err(error) = platform::open::open_url(&url)
					{
						tracing::warn!(%error, "could not open an embed link");
					}
					NewWindowResponse::Deny
				})
				.with_download_started_handler(|_, _| false)
				.with_permission_handler(|_| PermissionResponse::Deny)
				.build_as_child(&parent);
			match built {
				Ok(webview) => {
					self.frames.insert(
						id,
						Frame {
							popup,
							webview,
							rect: Rect::default(),
							clip: Rect::default(),
							shown: false,
							placed: None,
						},
					);
					self.update_timer();
				}
				Err(error) => {
					tracing::warn!(%error, "could not create an embed frame");
					let _ = unsafe { DestroyWindow(popup) };
				}
			}
		}

		fn owner_origin(&self) -> Option<POINT> {
			let visible = unsafe {
				IsWindowVisible(self.owner).as_bool()
					&& !IsIconic(self.owner).as_bool()
					&& GetForegroundWindow() == self.owner
			};
			if !visible {
				return None;
			}
			let mut origin = POINT::default();
			unsafe { ClientToScreen(self.owner, &mut origin) }
				.as_bool()
				.then_some(origin)
		}

		fn follow_owner(&mut self) {
			let origin = self.owner_origin();
			for frame in self.frames.values_mut() {
				match origin.filter(|_| frame.shown) {
					Some(origin) => frame.show(origin, self.owner),
					None => frame.hide(),
				}
			}
		}

		fn update_timer(&mut self) {
			let needed = !self.frames.is_empty();
			if needed == self.ticking {
				return;
			}
			self.ticking = needed;
			unsafe {
				if needed {
					SetTimer(Some(self.message_window), TICK, TICK_MS, None);
				} else {
					let _ = KillTimer(Some(self.message_window), TICK);
				}
			}
		}

		fn close_all(&mut self) {
			for (_, frame) in self.frames.drain() {
				frame.destroy();
			}
			self.update_timer();
		}
	}

	impl Frame {
		fn show(&mut self, origin: POINT, owner: HWND) {
			let placement = (self.clip, origin);
			if self.placed == Some(placement) {
				self.raise(owner);
				return;
			}
			let first = self.placed.is_none();
			self.placed = Some(placement);
			unsafe {
				let _ = SetWindowPos(
					self.popup,
					None,
					origin.x + self.clip.x,
					origin.y + self.clip.y,
					self.clip.width,
					self.clip.height,
					SWP_NOACTIVATE | SWP_NOZORDER,
				);
			}
			if let Err(error) = self.webview.set_bounds(bounds(self.rect, self.clip)) {
				tracing::debug!(%error, "could not move an embed frame");
			}
			if first {
				let _ = unsafe { ShowWindow(self.popup, SW_SHOWNOACTIVATE) };
			}
			self.raise(owner);
		}

		fn raise(&self, owner: HWND) {
			let above_owner = unsafe { GetWindow(owner, GW_HWNDPREV) }.is_ok_and(|above| above == self.popup);
			if above_owner {
				return;
			}
			let _ = unsafe {
				SetWindowPos(
					self.popup,
					Some(HWND_TOP),
					0,
					0,
					0,
					0,
					SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
				)
			};
		}

		fn hide(&mut self) {
			if self.placed.take().is_some() {
				let _ = unsafe { ShowWindow(self.popup, SW_HIDE) };
			}
		}

		fn destroy(self) {
			let Self { popup, webview, .. } = self;
			drop(webview);
			let _ = unsafe { DestroyWindow(popup) };
		}
	}
}

#[cfg(not(windows))]
mod host {
	use std::path::PathBuf;

	use super::Rect;

	pub struct Host;

	impl Host {
		pub fn start(_: isize, _: PathBuf) -> Option<Self> {
			None
		}

		pub fn open(&self, _: u64, _: String) {}

		pub fn place(&self, _: u64, _: Rect, _: Rect) {}

		pub fn hide(&self, _: u64) {}

		pub fn close(&self, _: u64) {}
	}
}

#[cfg(windows)]
pub fn owner(window: &gpui::Window) -> Option<isize> {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	match HasWindowHandle::window_handle(window).ok()?.as_raw() {
		RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
		_ => None,
	}
}

#[cfg(not(windows))]
pub fn owner(_: &gpui::Window) -> Option<isize> {
	None
}
