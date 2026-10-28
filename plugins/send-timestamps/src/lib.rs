use plugin_sdk::model::{Space, Tone, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, OutgoingMessage, Plugin, PluginExt, Reply, SendDecision, client, export_plugin, log, mount,
	now_ms, storage,
};

const REPLACE_KEY: &str = "replaceMessageContents";
const OFFSET_KEY: &str = "utcOffset";
const TOOLBAR_KEY: &str = "timestamp";
const OPEN_BUTTON: &str = "open";
const INSERT_BUTTON: &str = "insert";
const DATE_FIELD: &str = "date";
const TIME_FIELD: &str = "time";
const OFFSET_FIELD: &str = "offset";
const STYLE_FIELD: &str = "style";
const DEFAULT_STYLE: &str = "default";
const MODAL_POINT: &str = "modal";
const DAY_SECONDS: i64 = 86_400;
const STYLES: [(&str, &str); 10] = [
	(DEFAULT_STYLE, "Default"),
	("t", "Short time"),
	("T", "Long time"),
	("d", "Short date"),
	("D", "Long date"),
	("f", "Short date and time"),
	("F", "Long date and time"),
	("s", "Numeric date"),
	("S", "Numeric date and time"),
	("R", "Relative"),
];

struct SendTimestamps {
	replace_contents: bool,
	offset_minutes: i32,
	date: String,
	time: String,
	offset: String,
	style: String,
}

impl Default for SendTimestamps {
	fn default() -> Self {
		Self {
			replace_contents: true,
			offset_minutes: 0,
			date: String::new(),
			time: String::new(),
			offset: String::new(),
			style: DEFAULT_STYLE.to_owned(),
		}
	}
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
	let year = if month <= 2 { year - 1 } else { year };
	let era = year.div_euclid(400);
	let year_of_era = year.rem_euclid(400);
	let shifted_month = (month + 9) % 12;
	let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
	let shifted = days + 719_468;
	let era = shifted.div_euclid(146_097);
	let day_of_era = shifted.rem_euclid(146_097);
	let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let shifted_month = (5 * day_of_year + 2) / 153;
	let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
	let month = if shifted_month < 10 {
		shifted_month + 3
	} else {
		shifted_month - 9
	};
	let year = year_of_era + era * 400 + i64::from(month <= 2);
	(year, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
	match month {
		2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
		2 => 28,
		4 | 6 | 9 | 11 => 30,
		_ => 31,
	}
}

fn parse_offset(text: &str) -> Option<i32> {
	let text = text.trim();
	if text.is_empty() || text.eq_ignore_ascii_case("utc") || text.eq_ignore_ascii_case("z") {
		return Some(0);
	}
	let (sign, digits) = match text.as_bytes().first()? {
		b'-' => (-1, &text[1..]),
		b'+' => (1, &text[1..]),
		_ => (1, text),
	};
	let (hours, minutes) = match digits.split_once(':') {
		Some((hours, minutes)) => (hours, minutes),
		None if digits.len() > 2 => digits.split_at(digits.len() - 2),
		None => (digits, "0"),
	};
	let hours: i32 = hours.parse().ok()?;
	let minutes: i32 = minutes.parse().ok()?;
	(hours <= 14 && minutes < 60).then_some(sign * (hours * 60 + minutes))
}

fn format_offset(minutes: i32) -> String {
	let sign = if minutes < 0 { '-' } else { '+' };
	format!("{sign}{:02}:{:02}", minutes.abs() / 60, minutes.abs() % 60)
}

fn parse_clock(text: &str) -> Option<(i64, i64)> {
	let (hours, rest) = text.split_once(':')?;
	if hours.is_empty() || hours.len() > 2 || rest.len() < 2 || !rest.is_char_boundary(2) {
		return None;
	}
	let (minutes, suffix) = rest.split_at(2);
	let hours: i64 = hours.parse().ok()?;
	let minutes: i64 = minutes.parse().ok()?;
	if minutes > 59 {
		return None;
	}
	let suffix = suffix.strip_prefix(' ').unwrap_or(suffix);
	match suffix.to_ascii_lowercase().as_str() {
		"" => (hours <= 23).then_some((hours, minutes)),
		"am" => (1..=12).contains(&hours).then_some((hours % 12, minutes)),
		"pm" => (1..=12).contains(&hours).then_some((hours % 12 + 12, minutes)),
		_ => None,
	}
}

fn parse_date(text: &str) -> Option<i64> {
	let mut parts = text.trim().splitn(3, '-');
	let year: i64 = parts.next()?.parse().ok()?;
	let month: i64 = parts.next()?.parse().ok()?;
	let day: i64 = parts.next()?.parse().ok()?;
	((1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day))
		.then(|| days_from_civil(year, month, day))
}

fn local_clock(unix_seconds: i64, offset_minutes: i32) -> (String, String) {
	let local = unix_seconds + i64::from(offset_minutes) * 60;
	let (year, month, day) = civil_from_days(local.div_euclid(DAY_SECONDS));
	let seconds = local.rem_euclid(DAY_SECONDS);
	(
		format!("{year:04}-{month:02}-{day:02}"),
		format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60),
	)
}

fn token(unix_seconds: i64, style: &str) -> String {
	if style == DEFAULT_STYLE {
		format!("<t:{unix_seconds}>")
	} else {
		format!("<t:{unix_seconds}:{style}>")
	}
}

fn next_occurrence(hours: i64, minutes: i64, offset_minutes: i32, now_seconds: i64) -> i64 {
	let shift = i64::from(offset_minutes) * 60;
	let local_midnight = (now_seconds + shift).div_euclid(DAY_SECONDS) * DAY_SECONDS;
	let unix = local_midnight + hours * 3600 + minutes * 60 - shift;
	if unix < now_seconds {
		unix + DAY_SECONDS
	} else {
		unix
	}
}

fn replace_clock_times(content: &str, offset_minutes: i32, now_seconds: i64) -> String {
	let mut out = String::with_capacity(content.len());
	let mut rest = content;
	while let Some(open) = rest.find('`') {
		out.push_str(&rest[..open]);
		let after = &rest[open + 1..];
		let parsed = after
			.find('`')
			.and_then(|close| parse_clock(&after[..close]).map(|clock| (close, clock)));
		match parsed {
			Some((close, (hours, minutes))) => {
				out.push_str(&token(
					next_occurrence(hours, minutes, offset_minutes, now_seconds),
					"t",
				));
				rest = &after[close + 1..];
			}
			None => {
				out.push('`');
				rest = after;
			}
		}
	}
	out.push_str(rest);
	out
}

fn is_enabled(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl SendTimestamps {
	fn selected_unix(&self) -> Option<i64> {
		let offset = parse_offset(&self.offset)?;
		let (hours, minutes) = parse_clock(self.time.trim())?;
		let days = parse_date(&self.date)?;
		Some(days * DAY_SECONDS + hours * 3600 + minutes * 60 - i64::from(offset) * 60)
	}

	fn reset_form(&mut self) {
		let (date, time) = local_clock(now_ms() / 1000, self.offset_minutes);
		self.date = date;
		self.time = time;
		self.offset = format_offset(self.offset_minutes);
		self.style = DEFAULT_STYLE.to_owned();
	}

	fn modal(&self) -> Vec<Node> {
		let styles = STYLES
			.iter()
			.fold(view::select(STYLE_FIELD, "Style"), |select, (value, name)| {
				select.option(*value, *name)
			})
			.choose(self.style.as_str());
		let selected = self.selected_unix();
		let preview = match selected {
			Some(unix) => {
				let shown = if self.style == DEFAULT_STYLE {
					"f"
				} else {
					&self.style
				};
				view::row()
					.gap(Space::Sm)
					.child(view::timestamp(unix * 1000, shown))
					.child(view::text(token(unix, &self.style)).mono().muted())
			}
			None => view::text("Use YYYY-MM-DD, HH:MM and an offset like +02:00").muted(),
		};
		let insert = view::button(INSERT_BUTTON, "Insert")
			.icon("calendar")
			.tone(Tone::Brand)
			.disabled(selected.is_none());
		vec![
			view::column()
				.gap(Space::Md)
				.padding(Space::Md)
				.child(
					view::row()
						.gap(Space::Sm)
						.child(view::input(DATE_FIELD, "Date").value(self.date.as_str()))
						.child(view::input(TIME_FIELD, "Time").value(self.time.as_str()))
						.child(view::input(OFFSET_FIELD, "UTC offset").value(self.offset.as_str())),
				)
				.child(styles)
				.child(preview)
				.child(insert),
		]
	}

	fn change(&mut self, field: &str, value: String) {
		match field {
			DATE_FIELD => self.date = value,
			TIME_FIELD => self.time = value,
			OFFSET_FIELD => self.offset = value,
			STYLE_FIELD => self.style = value,
			_ => {}
		}
	}
}

impl Plugin for SendTimestamps {
	fn init(&mut self) -> Result<(), String> {
		let read = |key: &str| {
			storage::get(&format!("settings.{key}"))
				.ok()
				.flatten()
				.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
		};
		if let Some(raw) = read(REPLACE_KEY) {
			self.replace_contents = is_enabled(&raw);
		}
		if let Some(offset) = read(OFFSET_KEY).and_then(|raw| parse_offset(&raw)) {
			self.offset_minutes = offset;
		}
		let button = view::icon_button(OPEN_BUTTON, "calendar", "Insert timestamp");
		let options = mount::Options::new("Insert timestamp");
		if let Err(error) = mount::mount(mount::COMPOSER_TOOLBAR, TOOLBAR_KEY, [button], options) {
			log::warn!("could not add the composer button: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		let Event::SettingChanged(SettingChange { key, value }) = event else {
			return;
		};
		match key.as_str() {
			REPLACE_KEY => self.replace_contents = is_enabled(&value),
			OFFSET_KEY => {
				if let Some(offset) = parse_offset(&value) {
					self.offset_minutes = offset;
				}
			}
			_ => {}
		}
	}

	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		if !self.replace_contents || !msg.content.contains('`') {
			return SendDecision::Pass;
		}
		let content = replace_clock_times(&msg.content, self.offset_minutes, now_ms() / 1000);
		if content == msg.content {
			SendDecision::Pass
		} else {
			SendDecision::Replace(OutgoingMessage { content, ..msg })
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"replaceMessageContents":{"type":"boolean","title":"Replace timestamps in messages","description":"Turn a time in backticks, like `17:30` or `5:30 PM`, into a timestamp when you send","default":true},"utcOffset":{"type":"string","title":"UTC offset","description":"Your offset from UTC, like +02:00 or -05:00. Plugins cannot read the system time zone","default":"+00:00"}}}"#
			.to_owned()
	}
}

impl PluginExt for SendTimestamps {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		match (&event.kind, event.node.as_str()) {
			(ViewEventKind::Click, OPEN_BUTTON) => {
				self.reset_form();
				if let Err(error) = mount::open_modal("Timestamp", self.modal()) {
					log::warn!("could not open the timestamp picker: {error:?}");
				}
				Reply::none()
			}
			(ViewEventKind::Click, INSERT_BUTTON) if event.point == MODAL_POINT => {
				if let Some(unix) = self.selected_unix()
					&& let Err(error) = client::composer_insert(&format!("{} ", token(unix, &self.style)))
				{
					log::warn!("could not insert the timestamp: {error:?}");
				}
				Reply::none().close()
			}
			(ViewEventKind::Change(value) | ViewEventKind::Select(value), field)
				if event.point == MODAL_POINT =>
			{
				self.change(field, value.clone());
				Reply::none().replace(self.modal())
			}
			_ => Reply::none(),
		}
	}
}

export_plugin!(SendTimestamps, ext);
