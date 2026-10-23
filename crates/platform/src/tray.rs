use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::{PlatformError, Result};

static CREATED: AtomicBool = AtomicBool::new(false);

const OPEN_ID: &str = "oxidcord.open";
const MUTE_ID: &str = "oxidcord.mute";
const DEAFEN_ID: &str = "oxidcord.deafen";
const QUIT_ID: &str = "oxidcord.quit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayItem {
	Open,
	ToggleMute,
	ToggleDeafen,
	Separator,
	Quit,
	Custom { id: String, label: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
	Open,
	ToggleMute,
	ToggleDeafen,
	Quit,
	MenuId(String),
}

#[derive(Debug, Clone)]
pub struct TrayLabels {
	pub open: String,
	pub mute: String,
	pub deafen: String,
	pub quit: String,
}

#[derive(Debug, Clone)]
pub struct TrayConfig {
	pub tooltip: String,
	pub icon_rgba: (Vec<u8>, u32, u32),
	pub menu: Vec<TrayItem>,
	pub labels: TrayLabels,
}

pub struct Tray {
	icon: TrayIcon,
}

impl Tray {
	pub fn new(config: TrayConfig) -> Result<(Tray, Receiver<TrayEvent>)> {
		if CREATED.swap(true, Ordering::SeqCst) {
			return Err(PlatformError::AlreadyCreated("tray"));
		}
		let (icon_rgba, width, height) = config.icon_rgba;
		let (sender, receiver) = mpsc::channel();

		let menu_sender = sender.clone();
		MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
			let _ = menu_sender.send(menu_event(event.id.as_ref()));
		}));
		TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
			if let TrayIconEvent::Click {
				button: MouseButton::Left,
				button_state: MouseButtonState::Up,
				..
			} = event
			{
				let _ = sender.send(TrayEvent::Open);
			}
		}));

		let icon = TrayIconBuilder::new()
			.with_tooltip(config.tooltip)
			.with_icon(Icon::from_rgba(icon_rgba, width, height)?)
			.with_menu(Box::new(build_menu(&config.menu, &config.labels)?))
			.with_menu_on_left_click(false)
			.build()?;
		Ok((Tray { icon }, receiver))
	}

	pub fn set_tooltip(&self, tooltip: &str) -> Result<()> {
		self.icon.set_tooltip(Some(tooltip))?;
		Ok(())
	}
}

fn menu_event(id: &str) -> TrayEvent {
	match id {
		OPEN_ID => TrayEvent::Open,
		MUTE_ID => TrayEvent::ToggleMute,
		DEAFEN_ID => TrayEvent::ToggleDeafen,
		QUIT_ID => TrayEvent::Quit,
		other => TrayEvent::MenuId(other.to_owned()),
	}
}

fn build_menu(items: &[TrayItem], labels: &TrayLabels) -> Result<Menu> {
	let menu = Menu::new();
	for item in items {
		match item {
			TrayItem::Separator => menu.append(&PredefinedMenuItem::separator()),
			TrayItem::Open => menu.append(&MenuItem::with_id(OPEN_ID, &labels.open, true, None)),
			TrayItem::ToggleMute => menu.append(&MenuItem::with_id(MUTE_ID, &labels.mute, true, None)),
			TrayItem::ToggleDeafen => menu.append(&MenuItem::with_id(DEAFEN_ID, &labels.deafen, true, None)),
			TrayItem::Quit => menu.append(&MenuItem::with_id(QUIT_ID, &labels.quit, true, None)),
			TrayItem::Custom { id, label } => menu.append(&MenuItem::with_id(id.as_str(), label, true, None)),
		}
		.map_err(|error| PlatformError::Os(error.to_string()))?;
	}
	Ok(menu)
}
