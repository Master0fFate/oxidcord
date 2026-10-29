use plugin_sdk::model::{OutgoingFull, SendDecisionFull, Tone, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Event, Plugin, PluginExt, Reply, export_plugin, log, mount, storage};

const PERSIST_KEY: &str = "persistState";
const AUTO_DISABLE_KEY: &str = "autoDisable";
const SETTING_KEYS: [&str; 2] = [PERSIST_KEY, AUTO_DISABLE_KEY];
const TOOLBAR_KEY: &str = "silent";
const TOGGLE_BUTTON: &str = "toggle";

struct SilentMessageToggle {
	enabled: bool,
	persist_state: bool,
	auto_disable: bool,
}

impl Default for SilentMessageToggle {
	fn default() -> Self {
		Self {
			enabled: false,
			persist_state: false,
			auto_disable: true,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl SilentMessageToggle {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			PERSIST_KEY => self.persist_state = parse_flag(value),
			AUTO_DISABLE_KEY => self.auto_disable = parse_flag(value),
			_ => {}
		}
	}

	fn button(&self) -> Node {
		let (tone, tooltip) = if self.enabled {
			(Tone::Brand, "Disable silent message")
		} else {
			(Tone::Muted, "Enable silent message")
		};
		view::icon_button(TOGGLE_BUTTON, "bell-off", tooltip).tone(tone)
	}

	fn refresh_button(&self) {
		if let Err(error) = mount::update(mount::COMPOSER_TOOLBAR, TOOLBAR_KEY, [self.button()]) {
			log::debug!("could not update the toggle: {error:?}");
		}
	}

	fn disable(&mut self) {
		if std::mem::take(&mut self.enabled) {
			self.refresh_button();
		}
	}
}

impl Plugin for SilentMessageToggle {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		let options = mount::Options::new("Silent message");
		if let Err(error) = mount::mount(mount::COMPOSER_TOOLBAR, TOOLBAR_KEY, [self.button()], options) {
			log::warn!("could not add the composer button: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::SettingChanged(SettingChange { key, value }) => self.set(&key, &value),
			Event::ChannelSwitched(_) if !self.persist_state => self.disable(),
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"persistState":{"type":"boolean","title":"Persist state","description":"Keep the silent message toggle on when changing channels","default":false},"autoDisable":{"type":"boolean","title":"Auto disable","description":"Turn the silent message toggle off again after sending one","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for SilentMessageToggle {
	fn before_send_full(&mut self, msg: OutgoingFull) -> SendDecisionFull {
		if !self.enabled {
			return SendDecisionFull::Pass;
		}
		if self.auto_disable {
			self.disable();
		}
		SendDecisionFull::Replace(OutgoingFull { silent: true, ..msg })
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.point == mount::COMPOSER_TOOLBAR
			&& event.node == TOGGLE_BUTTON
			&& matches!(event.kind, ViewEventKind::Click)
		{
			self.enabled = !self.enabled;
			self.refresh_button();
		}
		Reply::none()
	}
}

export_plugin!(SilentMessageToggle, ext);
