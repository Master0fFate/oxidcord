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
		r#"{"type":"object","properties":{"sidebar":{"type":"boolean","title":"Folder sidebar","description":"Display servers from folder on dedicated sidebar","default":true},"sidebarAnim":{"type":"boolean","title":"Sidebar animation","description":"Not applicable, the folder sidebar does not animate","default":true},"closeAllFolders":{"type":"boolean","title":"Close all folders","description":"Not applicable, plugins cannot read or change which folders are open","default":false},"closeAllHomeButton":{"type":"boolean","title":"Close folders on Home","description":"Not applicable, plugins cannot read or change which folders are open","default":false},"closeOthers":{"type":"boolean","title":"Close other folders","description":"Not applicable, plugins cannot read or change which folders are open","default":false},"closeServerFolder":{"type":"boolean","title":"Close folder on server","description":"Not applicable, plugins cannot read or change which folders are open","default":false},"forceOpen":{"type":"boolean","title":"Force open","description":"Not applicable, plugins cannot read or change which folders are open","default":false},"keepIcons":{"type":"boolean","title":"Keep icons","description":"Keep showing guild icons in the server list folder when it is open in the folder sidebar","default":false},"showFolderIcon":{"type":"string","title":"Folder icon","description":"Not applicable, the folder sidebar has no folder icon above the servers","enum":["never","always","multiple"],"default":"always"}}}"#
			.to_owned()
	}
}

export_plugin!(BetterFolders);
