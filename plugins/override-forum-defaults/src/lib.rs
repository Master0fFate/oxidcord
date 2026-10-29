use plugin_sdk::model::ClientOption;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, actions, export_plugin, log, storage};

const LAYOUT_KEY: &str = "defaultLayout";
const SORT_KEY: &str = "defaultSortOrder";
const LIST_LAYOUT: u8 = 1;
const GALLERY_LAYOUT: u8 = 2;
const RECENT_SORT: u8 = 0;
const POSTED_SORT: u8 = 1;

struct OverrideForumDefaults {
	layout: u8,
	sort: u8,
}

impl Default for OverrideForumDefaults {
	fn default() -> Self {
		Self {
			layout: LIST_LAYOUT,
			sort: RECENT_SORT,
		}
	}
}

impl OverrideForumDefaults {
	fn set(&mut self, key: &str, value: &str) -> bool {
		match (key, value.trim()) {
			(LAYOUT_KEY, "gallery") => self.layout = GALLERY_LAYOUT,
			(LAYOUT_KEY, "list") => self.layout = LIST_LAYOUT,
			(SORT_KEY, "posted") => self.sort = POSTED_SORT,
			(SORT_KEY, "recent") => self.sort = RECENT_SORT,
			_ => return false,
		}
		true
	}

	fn apply(&self) {
		for option in [
			ClientOption::ForumLayout(self.layout),
			ClientOption::ForumSort(self.sort),
		] {
			if let Err(error) = actions::set_client_option(option) {
				log::warn!("could not change the forum default: {error:?}");
			}
		}
	}
}

impl Plugin for OverrideForumDefaults {
	fn init(&mut self) -> Result<(), String> {
		for key in [LAYOUT_KEY, SORT_KEY] {
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
		r#"{"type":"object","properties":{"defaultLayout":{"type":"string","title":"Layout","description":"Which layout to use as default: list or gallery","enum":["list","gallery"],"default":"list"},"defaultSortOrder":{"type":"string","title":"Sort order","description":"Which sort order to use as default: recent (recently active) or posted (date posted)","enum":["recent","posted"],"default":"recent"}}}"#
			.to_owned()
	}
}

export_plugin!(OverrideForumDefaults);
