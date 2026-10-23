use std::sync::mpsc::Receiver;
use std::thread;

use gpui::{Context, Global, Task, Window};
use platform::tray::{Tray, TrayConfig, TrayEvent, TrayItem, TrayLabels};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::app::AppRoot;
use crate::i18n::Translate as _;

const ICON_BYTES: &[u8] = include_bytes!("../../../assets/icon.png");

pub enum DesktopEvent {
	Show,
	ToggleMute,
	ToggleDeafen,
	Quit,
}

pub struct TrayActive;

impl Global for TrayActive {}

pub struct Desktop {
	_tray: Option<Tray>,
	_tasks: Vec<Task<()>>,
}

fn forward<T: Send + 'static>(source: Receiver<T>) -> UnboundedReceiver<T> {
	let (sender, receiver) = unbounded_channel();
	thread::spawn(move || {
		while let Ok(item) = source.recv() {
			if sender.send(item).is_err() {
				break;
			}
		}
	});
	receiver
}

fn tray_icon() -> Option<(Vec<u8>, u32, u32)> {
	let icon = image::load_from_memory(ICON_BYTES).ok()?.into_rgba8();
	let (width, height) = icon.dimensions();
	Some((icon.into_raw(), width, height))
}

fn desktop_event(event: TrayEvent) -> Option<DesktopEvent> {
	match event {
		TrayEvent::Open => Some(DesktopEvent::Show),
		TrayEvent::ToggleMute => Some(DesktopEvent::ToggleMute),
		TrayEvent::ToggleDeafen => Some(DesktopEvent::ToggleDeafen),
		TrayEvent::Quit => Some(DesktopEvent::Quit),
		TrayEvent::MenuId(_) => None,
	}
}

fn pump<T: Send + 'static>(
	receiver: UnboundedReceiver<T>,
	convert: fn(T) -> Option<DesktopEvent>,
	window: &mut Window,
	cx: &mut Context<AppRoot>,
) -> Task<()> {
	let mut receiver = receiver;
	cx.spawn_in(window, async move |this, cx| {
		while let Some(item) = receiver.recv().await {
			let Some(event) = convert(item) else {
				continue;
			};
			let delivered = this.update_in(cx, |root, window, cx| root.on_desktop(event, window, cx));
			if delivered.is_err() {
				break;
			}
		}
	})
}

impl Desktop {
	pub fn install(
		activation: Option<Receiver<Vec<u8>>>,
		window: &mut Window,
		cx: &mut Context<AppRoot>,
	) -> Self {
		let mut tasks = Vec::new();
		let tray = tray_icon().and_then(|icon_rgba| {
			let config = TrayConfig {
				tooltip: cx.t("app-name").to_string(),
				icon_rgba,
				menu: vec![
					TrayItem::Open,
					TrayItem::Separator,
					TrayItem::ToggleMute,
					TrayItem::ToggleDeafen,
					TrayItem::Separator,
					TrayItem::Quit,
				],
				labels: TrayLabels {
					open: cx.t("tray-open").to_string(),
					mute: cx.t("tray-mute").to_string(),
					deafen: cx.t("tray-deafen").to_string(),
					quit: cx.t("tray-quit").to_string(),
				},
			};
			match Tray::new(config) {
				Ok(pair) => Some(pair),
				Err(error) => {
					tracing::warn!(%error, "could not create the tray icon");
					None
				}
			}
		});
		let tray = tray.map(|(tray, events)| {
			cx.set_global(TrayActive);
			tasks.push(pump(forward(events), desktop_event, window, cx));
			tray
		});
		if let Some(activation) = activation {
			tasks.push(pump(
				forward(activation),
				|_| Some(DesktopEvent::Show),
				window,
				cx,
			));
		}
		Self {
			_tray: tray,
			_tasks: tasks,
		}
	}
}

#[cfg(windows)]
mod native {
	use gpui::Window;
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use windows::Win32::Foundation::HWND;
	use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SW_HIDE, SW_RESTORE, SW_SHOW, ShowWindow};

	fn handle(window: &Window) -> Option<HWND> {
		match HasWindowHandle::window_handle(window).ok()?.as_raw() {
			RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
			_ => None,
		}
	}

	pub fn hide(window: &Window) -> bool {
		let Some(hwnd) = handle(window) else {
			return false;
		};
		let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
		true
	}

	pub fn show(window: &Window) {
		let Some(hwnd) = handle(window) else {
			return;
		};
		let command = if unsafe { IsIconic(hwnd) }.as_bool() {
			SW_RESTORE
		} else {
			SW_SHOW
		};
		let _ = unsafe { ShowWindow(hwnd, command) };
	}
}

#[cfg(not(windows))]
mod native {
	use gpui::Window;

	pub fn hide(window: &Window) -> bool {
		window.minimize_window();
		true
	}

	pub fn show(_: &Window) {}
}

pub fn hide_window(window: &Window) -> bool {
	native::hide(window)
}

pub fn show_window(window: &Window) {
	native::show(window);
	window.activate_window();
}
