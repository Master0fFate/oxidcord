use std::collections::HashMap;

use plugin_sdk::model::{Badge, ClientEvent, Color, DisplayOverrideFull, Presence};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, User, actions, export_plugin, log, storage};

const SEEN_CAPACITY: usize = 4096;
const SETTING_KEYS: [&str; 2] = ["show", "colorMobileIndicator"];

struct Config {
	show: bool,
	color_mobile: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			show: true,
			color_mobile: true,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let on = !matches!(value.trim(), "false" | "0" | "off");
		match key {
			"show" => self.show = on,
			"colorMobileIndicator" => self.color_mobile = on,
			_ => {}
		}
	}
}

type Seen = (String, Vec<String>);

#[derive(Default)]
struct PlatformIndicators {
	config: Config,
	seen: HashMap<u64, Seen>,
}

fn icon(platform: &str) -> &'static str {
	match platform {
		"mobile" => "smartphone",
		"web" => "globe",
		"embedded" => "gamepad-2",
		_ => "monitor",
	}
}

fn tooltip(platform: &str) -> String {
	if platform == "embedded" {
		return "Console or game".to_owned();
	}
	let mut chars = platform.chars();
	chars
		.next()
		.map(|first| first.to_uppercase().chain(chars).collect())
		.unwrap_or_default()
}

fn status_color(status: &str) -> Color {
	Color::Token(
		match status {
			"online" => "success",
			"idle" => "warning",
			"dnd" => "danger",
			_ => "text-muted",
		}
		.to_owned(),
	)
}

impl PluginExt for PlatformIndicators {
	fn display_user_full(
		&mut self,
		_guild_id: Option<u64>,
		user: &User,
		presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		let presence = presence.filter(|_| !user.bot && self.config.show)?;
		if self.seen.len() >= SEEN_CAPACITY {
			self.seen.clear();
		}
		self.seen
			.insert(user.id, (presence.status.clone(), presence.platforms.clone()));
		if presence.platforms.is_empty() {
			return None;
		}
		let badges = presence
			.platforms
			.iter()
			.map(|platform| Badge {
				icon: Some(icon(platform).to_owned()),
				image: None,
				label: None,
				tooltip: tooltip(platform),
				color: (platform != "mobile" || self.config.color_mobile)
					.then(|| status_color(&presence.status)),
				key: None,
			})
			.collect();
		Some(DisplayOverrideFull {
			name: None,
			suffix: None,
			color: None,
			badges,
			avatar: None,
			banner: None,
			decoration: None,
			pronouns: None,
		})
	}

	fn on_client_event(&mut self, event: ClientEvent) {
		let ClientEvent::PresenceChanged(presence) = event else {
			return;
		};
		let Some(previous) = self.seen.get_mut(&presence.user_id) else {
			return;
		};
		if previous.0 == presence.status && previous.1 == presence.platforms {
			return;
		}
		*previous = (presence.status, presence.platforms);
		if let Err(error) = actions::invalidate_user(presence.user_id) {
			log::debug!("could not refresh user {}: {error:?}", presence.user_id);
		}
	}
}

impl Plugin for PlatformIndicators {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"show":{"type":"boolean","title":"Show indicators","description":"Show a platform icon next to the name of every user who is online","default":true},"colorMobileIndicator":{"type":"boolean","title":"Color mobile indicator","description":"Make the mobile indicator match the color of the user status","default":true}}}"#
			.to_owned()
	}
}

export_plugin!(PlatformIndicators, ext);
