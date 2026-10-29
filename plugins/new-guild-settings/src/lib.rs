use plugin_sdk::model::{ClientEvent, NotificationSettings};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, actions, export_plugin, log, storage};

const SETTING_KEYS: [&str; 4] = ["guild", "messages", "everyone", "role"];

struct NewGuildSettings {
	guild: bool,
	level: Option<u8>,
	everyone: bool,
	role: bool,
}

impl Default for NewGuildSettings {
	fn default() -> Self {
		Self {
			guild: true,
			level: None,
			everyone: true,
			role: true,
		}
	}
}

impl NewGuildSettings {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		let on = !matches!(value, "false" | "0" | "off");
		match key {
			"guild" => self.guild = on,
			"everyone" => self.everyone = on,
			"role" => self.role = on,
			"messages" => {
				self.level = match value {
					"all" => Some(0),
					"mentions" => Some(1),
					"nothing" => Some(2),
					_ => None,
				};
			}
			_ => {}
		}
	}

	fn settings(&self) -> NotificationSettings {
		NotificationSettings {
			muted: Some(self.guild),
			level: self.level,
			suppress_everyone: Some(self.everyone),
			suppress_roles: Some(self.role),
			mobile_push: None,
			hide_muted: None,
			mute_until_ms: None,
		}
	}
}

impl Plugin for NewGuildSettings {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"guild":{"type":"boolean","title":"Mute server","description":"Mute the server automatically","default":true},"messages":{"type":"string","title":"Message notifications","description":"Server notification settings, default leaves the server default alone","enum":["all","mentions","nothing","default"],"default":"default"},"everyone":{"type":"boolean","title":"Suppress @everyone and @here","description":"Suppress @everyone and @here","default":true},"role":{"type":"boolean","title":"Suppress role mentions","description":"Suppress all role mentions","default":true},"highlights":{"type":"boolean","title":"Suppress highlights","description":"Not applicable, the client has no setting for highlights","default":true},"events":{"type":"boolean","title":"Mute new events","description":"Not applicable, the client has no setting for scheduled events","default":true},"showAllChannels":{"type":"boolean","title":"Show all channels","description":"Not applicable, the client has no setting for showing all channels","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for NewGuildSettings {
	fn on_client_event(&mut self, event: ClientEvent) {
		if let ClientEvent::GuildJoined(guild_id) = event
			&& let Err(error) = actions::set_guild_notifications(guild_id, self.settings())
		{
			log::warn!("could not apply the settings to server {guild_id}: {error:?}");
		}
	}
}

export_plugin!(NewGuildSettings, ext);
