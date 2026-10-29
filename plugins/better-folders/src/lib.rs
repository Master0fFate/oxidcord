use plugin_sdk::model::ClientOption;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, actions, export_plugin, log, storage};

const SIDEBAR_KEY: &str = "sidebar";
const KEEP_ICONS_KEY: &str = "keepIcons";

struct BetterFolders {
	sidebar: bool,
	keep_icons: bool,
}

impl Default for BetterFolders {
	fn default() -> Self {
		Self {
			sidebar: true,
			keep_icons: false,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl BetterFolders {
	fn set(&mut self, key: &str, value: &str) -> bool {
		match key {
			SIDEBAR_KEY => self.sidebar = parse_flag(value),
			KEEP_ICONS_KEY => self.keep_icons = parse_flag(value),
			_ => return false,
		}
		true
	}

	fn apply(&self) {
		for option in [
			ClientOption::FolderSidebar(self.sidebar),
			ClientOption::PlainFolderIcons(self.sidebar && !self.keep_icons),
		] {
			if let Err(error) = actions::set_client_option(option) {
				log::warn!("could not change the folder display: {error:?}");
			}
		}
	}
}

impl Plugin for BetterFolders {
	fn init(&mut self) -> Result<(), String> {
		for key in [SIDEBAR_KEY, KEEP_ICONS_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.apply();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& self.set(&key, &value)
		{
			self.apply();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"sidebar":{"type":"boolean","title":"Folder sidebar","description":"Show the servers of an open folder in a sidebar next to the server list","default":true},"keepIcons":{"type":"boolean","title":"Keep icons","description":"Keep the small server icons on a folder in the server list while it is open in the sidebar","default":false}}}"#
			.to_owned()
	}
}

export_plugin!(BetterFolders);
