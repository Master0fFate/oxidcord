use plugin_sdk::model::{MessageFull, MessageMeta};
use plugin_sdk::patch::PatchExt;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, Plugin, PluginExt, export_plugin, storage};

const OFFSET_KEY: &str = "utcOffset";
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;
const DAY_SECONDS: i64 = 86_400;

#[derive(Default)]
struct ReplyTimestamp {
	offset_minutes: i32,
}

fn parse_offset(text: &str) -> Option<i32> {
	let text = text.trim();
	let (sign, rest) = match text.as_bytes().first()? {
		b'+' => (1, &text[1..]),
		b'-' => (-1, &text[1..]),
		_ => (1, text),
	};
	let (hours, minutes) = rest.split_once(':').unwrap_or((rest, "0"));
	let hours: i32 = hours.parse().ok()?;
	let minutes: i32 = minutes.parse().ok()?;
	(hours <= 14 && minutes < 60).then_some(sign * (hours * 60 + minutes))
}

fn snowflake_seconds(id: u64) -> i64 {
	((id >> 22) as i64 + DISCORD_EPOCH_MS) / 1000
}

fn civil_date(days: i64) -> (i64, i64, i64) {
	let shifted = days + 719_468;
	let era = shifted.div_euclid(146_097);
	let day_of_era = shifted.rem_euclid(146_097);
	let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let month_index = (5 * day_of_year + 2) / 153;
	let day = day_of_year - (153 * month_index + 2) / 5 + 1;
	let month = if month_index < 10 {
		month_index + 3
	} else {
		month_index - 9
	};
	let year = year_of_era + era * 400 + i64::from(month <= 2);
	(year, month, day)
}

fn short_time(seconds_of_day: i64) -> String {
	let hours = seconds_of_day / 3600;
	let minutes = seconds_of_day % 3600 / 60;
	let (shown, suffix) = match hours {
		0 => (12, "AM"),
		1..=11 => (hours, "AM"),
		12 => (12, "PM"),
		_ => (hours - 12, "PM"),
	};
	format!("{shown}:{minutes:02} {suffix}")
}

impl ReplyTimestamp {
	fn label(&self, reply_to: u64, sent_ms: i64) -> String {
		let shift = i64::from(self.offset_minutes) * 60;
		let local = snowflake_seconds(reply_to) + shift;
		let reply_day = local.div_euclid(DAY_SECONDS);
		let time = short_time(local.rem_euclid(DAY_SECONDS));
		if reply_day == (sent_ms / 1000 + shift).div_euclid(DAY_SECONDS) {
			return time;
		}
		let (year, month, day) = civil_date(reply_day);
		format!("{year}-{month:02}-{day:02} {time}")
	}
}

impl Plugin for ReplyTimestamp {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{OFFSET_KEY}"))
			&& let Some(offset) = parse_offset(&String::from_utf8_lossy(&saved))
		{
			self.offset_minutes = offset;
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == OFFSET_KEY
			&& let Some(offset) = parse_offset(&value)
		{
			self.offset_minutes = offset;
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"utcOffset":{"type":"string","title":"UTC offset","description":"Your offset from UTC, like +02:00 or -05:00. Plugins cannot read the system time zone","default":"+00:00"}}}"#
			.to_owned()
	}
}

impl PluginExt for ReplyTimestamp {
	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		match &meta.reply_to {
			Some(reply) => PatchExt::keep()
				.reply_annotation(format!("[{}]", self.label(reply.id, msg.view.timestamp_ms))),
			None => PatchExt::keep(),
		}
	}
}

export_plugin!(ReplyTimestamp, ext);
