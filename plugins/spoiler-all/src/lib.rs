use plugin_sdk::types::SettingChange;
use plugin_sdk::ui::{self, button};
use plugin_sdk::{Event, Md, MessageView, Plugin, Ui, export_plugin, log, storage};

const ENABLED_KEY: &str = "enabled";
const TOGGLE_ACTION: &str = "toggle-spoiler";
const TOOLBAR_SLOT: &str = "composer-toolbar";

#[derive(Default)]
struct SpoilerAll {
	enabled: bool,
}

impl SpoilerAll {
	fn toolbar(&self) -> Vec<Ui> {
		let (label, icon) = if self.enabled {
			("Spoilers on", "eye-off")
		} else {
			("Spoilers off", "eye")
		};
		vec![button(TOGGLE_ACTION, label).icon(icon).into()]
	}

	fn set_enabled(&mut self, enabled: bool) {
		self.enabled = enabled;
		if let Err(error) = storage::set(ENABLED_KEY, if enabled { b"1" } else { b"0" }) {
			log::warn!("could not save the setting: {error:?}");
		}
	}

	fn publish_toolbar(&self) {
		if let Err(error) = ui::register_slot(TOOLBAR_SLOT, &self.toolbar()) {
			log::warn!("could not register the {TOOLBAR_SLOT} slot: {error:?}");
		}
	}
}

impl Plugin for SpoilerAll {
	fn init(&mut self) -> Result<(), String> {
		self.enabled = matches!(storage::get(ENABLED_KEY), Ok(Some(value)) if value == b"1");
		self.publish_toolbar();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == ENABLED_KEY
		{
			self.set_enabled(value == "true");
			self.publish_toolbar();
		}
	}

	fn render_message(&mut self, _msg: &MessageView, ast: Vec<Md>) -> Option<Vec<Md>> {
		let already_hidden = matches!(ast.as_slice(), [Md::Spoiler(_)]);
		(self.enabled && !ast.is_empty() && !already_hidden).then(|| vec![Md::Spoiler(ast)])
	}

	fn on_ui_action(&mut self, id: &str, _payload: &str) -> Vec<Ui> {
		if id != TOGGLE_ACTION {
			return Vec::new();
		}
		self.set_enabled(!self.enabled);
		self.toolbar()
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"enabled":{"type":"boolean","title":"Wrap every message in a spoiler","default":false}}}"#
			.to_owned()
	}
}

export_plugin!(SpoilerAll);
