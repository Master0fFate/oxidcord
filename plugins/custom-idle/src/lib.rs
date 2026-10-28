use plugin_sdk::model::{Align, Space, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{
	Event, Plugin, PluginExt, Reply, actions, data, export_plugin, host, log, mount, storage, timer,
};

const CHECK_MS: u32 = 30_000;
const CHECK_TAG: &str = "check";
const TIMEOUT_KEY: &str = "idleTimeout";
const REMAIN_KEY: &str = "remainInIdle";
const DEFAULT_TIMEOUT_MINUTES: u32 = 10;
const MAX_TIMEOUT_MINUTES: u32 = 60;
const EXIT_BUTTON: &str = "exit-idle";
const NOTICE_MS: u32 = 15_000;

struct CustomIdle {
	timeout_minutes: u32,
	remain_in_idle: bool,
	restore: Option<String>,
	welcomed: bool,
	me: Option<u64>,
}

impl Default for CustomIdle {
	fn default() -> Self {
		Self {
			timeout_minutes: DEFAULT_TIMEOUT_MINUTES,
			remain_in_idle: true,
			restore: None,
			welcomed: false,
			me: None,
		}
	}
}

fn parse_timeout(raw: &str) -> u32 {
	raw.trim()
		.parse::<f64>()
		.map_or(DEFAULT_TIMEOUT_MINUTES, |minutes| {
			minutes.clamp(0., f64::from(MAX_TIMEOUT_MINUTES)) as u32
		})
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl CustomIdle {
	fn own_status(&mut self) -> Option<String> {
		if self.me.is_none() {
			self.me = host::current_user().ok().map(|user| user.id);
		}
		self.me
			.and_then(|id| data::user_presence(id).ok().flatten())
			.map(|presence| presence.status)
	}

	fn set_status(status: &str) {
		if let Err(error) = actions::set_status(status) {
			log::warn!("could not set the status to {status}: {error:?}");
		}
	}

	fn leave_idle(&mut self) {
		if let Some(previous) = self.restore.take() {
			Self::set_status(&previous);
		}
		self.welcomed = false;
	}

	fn welcome_back(&mut self) {
		self.welcomed = true;
		let notice = view::row()
			.gap(Space::Md)
			.align(Align::Center)
			.child(view::text("Welcome back, you are still idle"))
			.child(view::button(EXIT_BUTTON, "Go online"));
		if let Err(error) = mount::toast([notice], NOTICE_MS) {
			log::debug!("welcome back notice skipped: {error:?}");
		}
	}

	fn go_idle(&mut self) {
		let status = self.own_status().unwrap_or_else(|| "online".to_owned());
		if status == "online" {
			Self::set_status("idle");
			self.restore = Some(status);
		}
	}

	fn watch_idle(&mut self, away_seconds: u32, limit_seconds: u32) {
		if self.own_status().is_some_and(|status| status != "idle") {
			self.restore = None;
			self.welcomed = false;
		} else if limit_seconds == 0 {
			self.leave_idle();
		} else if away_seconds < limit_seconds {
			if !self.remain_in_idle {
				self.leave_idle();
			} else if !self.welcomed {
				self.welcome_back();
			}
		}
	}

	fn check(&mut self) {
		let Ok(away_seconds) = data::idle_seconds() else {
			return;
		};
		let limit_seconds = self.timeout_minutes.saturating_mul(60);
		if self.restore.is_some() {
			self.watch_idle(away_seconds, limit_seconds);
		} else if limit_seconds > 0 && away_seconds >= limit_seconds {
			self.go_idle();
		}
	}
}

impl Plugin for CustomIdle {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{TIMEOUT_KEY}")) {
			self.timeout_minutes = parse_timeout(&String::from_utf8_lossy(&saved));
		}
		if let Ok(Some(saved)) = storage::get(&format!("settings.{REMAIN_KEY}")) {
			self.remain_in_idle = parse_flag(&String::from_utf8_lossy(&saved));
		}
		if let Err(error) = timer::every(CHECK_MS, CHECK_TAG) {
			log::warn!("could not start the idle timer: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		let Event::SettingChanged(SettingChange { key, value }) = event else {
			return;
		};
		match key.as_str() {
			TIMEOUT_KEY => self.timeout_minutes = parse_timeout(&value),
			REMAIN_KEY => self.remain_in_idle = parse_flag(&value),
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"idleTimeout":{"type":"integer","title":"Idle after","description":"Minutes without input before your status goes idle, 0 turns it off","minimum":0,"maximum":60,"default":10},"remainInIdle":{"type":"boolean","title":"Stay idle","description":"When you come back, stay idle until you confirm you want to go online","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for CustomIdle {
	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == CHECK_TAG {
			self.check();
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.node == EXIT_BUTTON && event.kind == ViewEventKind::Click {
			self.leave_idle();
		}
		Reply::none()
	}
}

export_plugin!(CustomIdle, ext);
