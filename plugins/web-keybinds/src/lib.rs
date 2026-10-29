use plugin_sdk::model::{Keybind, Space, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{Event, Plugin, PluginExt, Reply, actions, export_plugin, log, mount, storage};

const BUTTONS_KEY: &str = "showNavigationButtons";
const MOUNT_KEY: &str = "navigation";
const BACK: &str = "back";
const FORWARD: &str = "forward";
const SETTINGS: &str = "settings";
const SETTINGS_PAGE: &str = "my-account";
const BINDINGS: [(&str, &str); 3] = [(SETTINGS, "ctrl+,"), (BACK, "alt+left"), (FORWARD, "alt+right")];

struct WebKeybinds {
	show_buttons: bool,
}

impl Default for WebKeybinds {
	fn default() -> Self {
		Self { show_buttons: true }
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl WebKeybinds {
	fn sync_buttons(&self) {
		let outcome = if self.show_buttons {
			let buttons = view::row()
				.gap(Space::Xs)
				.child(view::icon_button(BACK, "arrow-left", "Back"))
				.child(view::icon_button(FORWARD, "arrow-right", "Forward"));
			mount::mount(
				mount::TITLE_BAR,
				MOUNT_KEY,
				[buttons],
				mount::Options::new("Navigation"),
			)
		} else {
			mount::unmount(mount::TITLE_BAR, MOUNT_KEY)
		};
		if let Err(error) = outcome {
			log::debug!("could not update the navigation buttons: {error:?}");
		}
	}
}

fn run(id: &str) {
	let outcome = match id {
		SETTINGS => actions::open_settings(SETTINGS_PAGE),
		BACK => actions::navigate_back(),
		FORWARD => actions::navigate_forward(),
		_ => return,
	};
	if let Err(error) = outcome {
		log::debug!("navigation failed: {error:?}");
	}
}

impl Plugin for WebKeybinds {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{BUTTONS_KEY}")) {
			self.show_buttons = parse_flag(&String::from_utf8_lossy(&saved));
		}
		for (id, chord) in BINDINGS {
			let bind = Keybind {
				id: id.to_owned(),
				chord: chord.to_owned(),
			};
			if let Err(error) = actions::register_keybind(&bind) {
				log::warn!("could not bind {chord}: {error:?}");
			}
		}
		self.sync_buttons();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == BUTTONS_KEY
		{
			self.show_buttons = parse_flag(&value);
			self.sync_buttons();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"showNavigationButtons":{"type":"boolean","title":"Show navigation buttons","description":"Show the back and forward buttons in the title bar","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for WebKeybinds {
	fn on_keybind(&mut self, id: &str) {
		run(id);
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if matches!(event.kind, ViewEventKind::Click) {
			run(&event.node);
		}
		Reply::none()
	}
}

export_plugin!(WebKeybinds, ext);
