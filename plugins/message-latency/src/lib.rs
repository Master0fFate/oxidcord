use plugin_sdk::model::{MessageFull, MessageMeta, Tone};
use plugin_sdk::patch::{Patch, PatchExt};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, Plugin, PluginExt, export_plugin, storage};

const LATENCY_KEY: &str = "latency";
const SHOW_MILLIS_KEY: &str = "showMillis";
const IGNORE_SELF_KEY: &str = "ignoreSelf";
const DEFAULT_THRESHOLD_SECONDS: f64 = 2.;
const TROLL_LIMIT_MS: u64 = 2 * 60 * 1000;
const UNITS: [(&str, u64); 5] = [
	("day", 86_400_000),
	("hour", 3_600_000),
	("minute", 60_000),
	("second", 1_000),
	("millisecond", 1),
];

struct MessageLatency {
	threshold_seconds: f64,
	show_millis: bool,
	ignore_self: bool,
}

impl Default for MessageLatency {
	fn default() -> Self {
		Self {
			threshold_seconds: DEFAULT_THRESHOLD_SECONDS,
			show_millis: false,
			ignore_self: false,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

fn parse_threshold(raw: &str) -> f64 {
	raw.trim()
		.parse::<f64>()
		.map_or(DEFAULT_THRESHOLD_SECONDS, |value| value.max(0.))
}

fn describe(mut delta_ms: u64, show_millis: bool) -> String {
	let mut parts: Vec<String> = Vec::new();
	let units = if show_millis { &UNITS[..] } else { &UNITS[..4] };
	for (name, size) in units {
		let count = delta_ms / size;
		delta_ms %= size;
		if count > 0 {
			parts.push(format!("{count} {name}{}", if count > 1 { "s" } else { "" }));
		}
	}
	match parts.pop() {
		None => "0 seconds".to_owned(),
		Some(last) if parts.is_empty() => last,
		Some(last) => format!("{} and {last}", parts.join(" ")),
	}
}

impl MessageLatency {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			LATENCY_KEY => self.threshold_seconds = parse_threshold(value),
			SHOW_MILLIS_KEY => self.show_millis = parse_flag(value),
			IGNORE_SELF_KEY => self.ignore_self = parse_flag(value),
			_ => {}
		}
	}

	fn tone(&self, delta_ms: u64, threshold_ms: u64) -> Tone {
		if delta_ms >= TROLL_LIMIT_MS {
			Tone::Muted
		} else if delta_ms >= threshold_ms * 2 {
			Tone::Danger
		} else {
			Tone::Warning
		}
	}
}

impl Plugin for MessageLatency {
	fn init(&mut self) -> Result<(), String> {
		for key in [LATENCY_KEY, SHOW_MILLIS_KEY, IGNORE_SELF_KEY] {
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
		r#"{"type":"object","properties":{"latency":{"type":"number","title":"Threshold","description":"Seconds a message must have taken to send before the line shows","minimum":0,"default":2},"showMillis":{"type":"boolean","title":"Show milliseconds","description":"Show milliseconds in the delay","default":false},"ignoreSelf":{"type":"boolean","title":"Ignore your messages","description":"Only your own messages have a measured delay, so turning this on hides every line","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for MessageLatency {
	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		let Some(latency) = meta.latency_ms else {
			return PatchExt::keep();
		};
		if self.ignore_self && msg.mine {
			return PatchExt::keep();
		}
		let measured = u64::from(latency);
		let delta_ms = if self.show_millis {
			measured
		} else {
			(measured + 500) / 1000 * 1000
		};
		let threshold_ms = (self.threshold_seconds * 1000.) as u64;
		if delta_ms < threshold_ms {
			return PatchExt::keep();
		}
		let text = format!("Sent with a delay of {}", describe(delta_ms, self.show_millis));
		PatchExt::keep().base(Patch::keep().annotate_with(
			text,
			"signal-low",
			self.tone(delta_ms, threshold_ms),
		))
	}
}

export_plugin!(MessageLatency, ext);
