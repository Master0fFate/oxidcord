use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{Event, Plugin, PluginExt, data, export_plugin, host, log, mount, storage, timer};

const FORMAT_KEY: &str = "format";
const MOUNT_KEY: &str = "elapsed";
const TICK_TAG: &str = "tick";
const TICK_MS: u32 = 1_000;
const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Format {
	#[default]
	Stopwatch,
	Human,
}

impl Format {
	fn parse(raw: &str) -> Self {
		match raw.trim() {
			"human" => Self::Human,
			_ => Self::Stopwatch,
		}
	}

	fn render(self, seconds: i64) -> String {
		let (days, hours, minutes, seconds) = (
			seconds / DAY,
			seconds % DAY / HOUR,
			seconds % HOUR / MINUTE,
			seconds % MINUTE,
		);
		match self {
			Self::Stopwatch if days > 0 => format!("{days}d {hours:02}:{minutes:02}:{seconds:02}"),
			Self::Stopwatch if hours > 0 => format!("{hours}:{minutes:02}:{seconds:02}"),
			Self::Stopwatch => format!("{minutes}:{seconds:02}"),
			Self::Human => [(days, 'd'), (hours, 'h'), (minutes, 'm'), (seconds, 's')]
				.into_iter()
				.skip_while(|&(amount, unit)| amount == 0 && unit != 's')
				.map(|(amount, unit)| format!("{amount}{unit}"))
				.collect::<Vec<_>>()
				.join(" "),
		}
	}
}

#[derive(Default)]
struct CallTimer {
	format: Format,
	ticker: Option<u64>,
}

fn show(text: &str) {
	let update = mount::update(mount::VOICE_PANEL, MOUNT_KEY, [view::text(text).mono()]);
	if let Err(error) = update {
		log::debug!("could not update the call timer: {error:?}");
	}
}

impl CallTimer {
	fn start(&mut self) {
		if self.ticker.is_some() {
			return;
		}
		match timer::every(TICK_MS, TICK_TAG) {
			Ok(id) => self.ticker = Some(id),
			Err(error) => log::warn!("could not start the call timer: {error:?}"),
		}
		self.tick();
	}

	fn stop(&mut self) {
		if let Some(id) = self.ticker.take() {
			timer::clear(id);
		}
	}

	fn tick(&mut self) {
		match data::voice_detail() {
			Ok(Some(detail)) => {
				let elapsed = if detail.started_ms > 0 {
					(host::now_ms() - detail.started_ms) / 1000
				} else {
					0
				};
				show(&self.format.render(elapsed.max(0)));
			}
			Ok(None) => self.stop(),
			Err(error) => log::debug!("could not read the call: {error:?}"),
		}
	}
}

impl Plugin for CallTimer {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{FORMAT_KEY}")) {
			self.format = Format::parse(&String::from_utf8_lossy(&saved));
		}
		let mounted = mount::mount(
			mount::VOICE_PANEL,
			MOUNT_KEY,
			[view::text(self.format.render(0)).mono()],
			mount::Options::new("Call timer"),
		);
		if let Err(error) = mounted {
			log::warn!("could not mount the call timer: {error:?}");
		}
		if matches!(data::voice_detail(), Ok(Some(_))) {
			self.start();
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::VoiceChanged(_) => self.start(),
			Event::SettingChanged(SettingChange { key, value }) if key == FORMAT_KEY => {
				self.format = Format::parse(&value);
				if self.ticker.is_some() {
					self.tick();
				}
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"format":{"type":"string","title":"Format","description":"The timer format: stopwatch (1:02:03) or human (1h 2m 3s)","enum":["stopwatch","human"],"default":"stopwatch"}}}"#
			.to_owned()
	}
}

impl PluginExt for CallTimer {
	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == TICK_TAG {
			self.tick();
		}
	}
}

export_plugin!(CallTimer, ext);
