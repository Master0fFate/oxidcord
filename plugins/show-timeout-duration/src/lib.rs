use std::collections::HashSet;

use plugin_sdk::model::{Badge, Color, DisplayOverrideFull, Presence};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, User, actions, data, export_plugin, log, now_ms, storage, timer};

const STYLE_KEY: &str = "displayStyle";
const REFRESH_TAG: &str = "refresh";
const REFRESH_MS: u32 = 60_000;
const TRACKED_USERS: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Style {
	Tooltip,
	#[default]
	Inline,
}

#[derive(Default)]
struct ShowTimeoutDuration {
	style: Style,
	timed_out: HashSet<u64>,
	refresh_armed: bool,
}

fn parse_style(raw: &str) -> Style {
	if raw.trim() == "tooltip" {
		Style::Tooltip
	} else {
		Style::Inline
	}
}

fn remaining(milliseconds: i64) -> String {
	let minutes = milliseconds / 60_000;
	match minutes {
		0 => "<1m".to_owned(),
		1..=59 => format!("{minutes}m"),
		60..=1_439 => format!("{}h {}m", minutes / 60, minutes % 60),
		_ => format!("{}d {}h", minutes / 1_440, minutes % 1_440 / 60),
	}
}

fn clock_badge(tooltip: String) -> Badge {
	Badge {
		icon: Some("clock".to_owned()),
		image: None,
		label: None,
		tooltip,
		color: Some(Color::Token("danger".to_owned())),
		key: None,
	}
}

impl ShowTimeoutDuration {
	fn track(&mut self, user_id: u64) {
		if self.timed_out.len() < TRACKED_USERS {
			self.timed_out.insert(user_id);
		}
		if !self.refresh_armed {
			match timer::once(REFRESH_MS, REFRESH_TAG) {
				Ok(_) => self.refresh_armed = true,
				Err(error) => log::debug!("could not schedule the timeout refresh: {error:?}"),
			}
		}
	}
}

impl Plugin for ShowTimeoutDuration {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{STYLE_KEY}")) {
			self.style = parse_style(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == STYLE_KEY
		{
			self.style = parse_style(&value);
			for user_id in self.timed_out.drain() {
				let _ = actions::invalidate_user(user_id);
			}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"displayStyle":{"type":"string","title":"Display style","description":"How to display the timeout duration: in the tooltip of the clock badge, or inline next to the name","enum":["tooltip","inline"],"default":"inline"}}}"#
			.to_owned()
	}
}

impl PluginExt for ShowTimeoutDuration {
	fn display_user_full(
		&mut self,
		guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		let until = data::member_info(guild_id?, user.id)
			.ok()
			.flatten()?
			.timeout_until_ms?;
		let left = until - now_ms();
		if left <= 0 {
			return None;
		}
		self.track(user.id);
		let left = remaining(left);
		let (tooltip, suffix) = match self.style {
			Style::Tooltip => (format!("{left} timeout remaining"), None),
			Style::Inline => ("Timed out".to_owned(), Some(format!("{left} left"))),
		};
		Some(DisplayOverrideFull {
			name: None,
			suffix,
			color: None,
			badges: vec![clock_badge(tooltip)],
			avatar: None,
			banner: None,
			decoration: None,
			pronouns: None,
		})
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag != REFRESH_TAG {
			return;
		}
		self.refresh_armed = false;
		for user_id in self.timed_out.drain() {
			if let Err(error) = actions::invalidate_user(user_id) {
				log::debug!("could not refresh user {user_id}: {error:?}");
			}
		}
	}
}

export_plugin!(ShowTimeoutDuration, ext);
