use plugin_sdk::model::ClientOption;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, actions, export_plugin, log, storage};

const VOLUME_KEY: &str = "notificationVolume";
const DEFAULT_VOLUME: u8 = 100;

struct NotificationVolume {
	volume: u8,
}

impl Default for NotificationVolume {
	fn default() -> Self {
		Self {
			volume: DEFAULT_VOLUME,
		}
	}
}

fn parse_volume(raw: &str) -> u8 {
	raw.trim()
		.parse::<f64>()
		.map_or(DEFAULT_VOLUME, |volume| volume.clamp(0., 100.) as u8)
}

impl NotificationVolume {
	fn apply(&self) {
		if let Err(error) = actions::set_client_option(ClientOption::NotificationVolume(self.volume)) {
			log::warn!("could not set the notification volume: {error:?}");
		}
	}
}

impl Plugin for NotificationVolume {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{VOLUME_KEY}")) {
			self.volume = parse_volume(&String::from_utf8_lossy(&saved));
		}
		self.apply();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == VOLUME_KEY
		{
			self.volume = parse_volume(&value);
			self.apply();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"notificationVolume":{"type":"integer","title":"Notification volume","description":"Notification volume","minimum":0,"maximum":100,"default":100}}}"#
			.to_owned()
	}
}

export_plugin!(NotificationVolume);
