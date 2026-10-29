use plugin_sdk::model::{TextSize, Tone};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, export_plugin, log, mount, storage, view};

const COLOR_EFFECTS_KEY: &str = "colorEffects";
const LIMIT_KEY: &str = "limit";
const SETTING_KEYS: [&str; 2] = [COLOR_EFFECTS_KEY, LIMIT_KEY];
const MOUNT_KEY: &str = "counter";
const WARNING_PERCENT: usize = 50;
const DANGER_PERCENT: usize = 90;

struct CharacterCounter {
	color_effects: bool,
	limit: usize,
	count: usize,
	mounted: bool,
}

impl Default for CharacterCounter {
	fn default() -> Self {
		Self {
			color_effects: true,
			limit: 2000,
			count: 0,
			mounted: false,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl CharacterCounter {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			COLOR_EFFECTS_KEY => self.color_effects = parse_flag(value),
			LIMIT_KEY => {
				if let Ok(limit) = value.trim().parse::<usize>() {
					self.limit = limit.max(1);
				}
			}
			_ => {}
		}
	}

	fn tone(&self) -> Tone {
		let percent = self.count * 100 / self.limit;
		if !self.color_effects || percent < WARNING_PERCENT {
			Tone::Muted
		} else if percent < DANGER_PERCENT {
			Tone::Warning
		} else {
			Tone::Danger
		}
	}

	fn redraw(&mut self) {
		if self.count == 0 {
			if std::mem::take(&mut self.mounted)
				&& let Err(error) = mount::unmount(mount::STATUS_BAR, MOUNT_KEY)
			{
				log::debug!("could not hide the counter: {error:?}");
			}
			return;
		}
		let label = view::text(format!("{}/{}", self.count, self.limit))
			.size(TextSize::Xs)
			.tone(self.tone());
		let outcome = if self.mounted {
			mount::update(mount::STATUS_BAR, MOUNT_KEY, [label])
		} else {
			mount::mount(
				mount::STATUS_BAR,
				MOUNT_KEY,
				[label],
				mount::Options::new("Character counter"),
			)
		};
		match outcome {
			Ok(()) => self.mounted = true,
			Err(error) => log::debug!("could not draw the counter: {error:?}"),
		}
	}
}

impl Plugin for CharacterCounter {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& SETTING_KEYS.contains(&key.as_str())
		{
			self.set(&key, &value);
			self.redraw();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"colorEffects":{"type":"boolean","title":"Color effects","description":"Enable warning and danger colors as you get closer to the character limit","default":true},"limit":{"type":"integer","title":"Character limit","description":"2000, or 4000 with Nitro. Plugins cannot see whether the account has Nitro","minimum":1,"maximum":4000,"default":2000}}}"#
			.to_owned()
	}
}

impl PluginExt for CharacterCounter {
	fn on_draft_change(&mut self, _channel_id: u64, text: &str) {
		let count = text.encode_utf16().count();
		if count != self.count {
			self.count = count;
			self.redraw();
		}
	}
}

export_plugin!(CharacterCounter, ext);
