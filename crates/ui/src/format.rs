use chrono::{DateTime, Datelike, Local, TimeDelta, Utc};
use gpui::{App, SharedString};
use markdown::TimestampStyle;

use crate::i18n::{ArgValue, Translate as _};

const KIB: f64 = 1024.;

pub fn clock(time: DateTime<Utc>) -> String {
	time.with_timezone(&Local).format("%-I:%M %p").to_string()
}

pub fn day_label(day: chrono::NaiveDate) -> String {
	day.format("%B %-d, %Y").to_string()
}

pub fn message_time(time: DateTime<Utc>, cx: &App) -> SharedString {
	let local = time.with_timezone(&Local);
	let today = Local::now().date_naive();
	let clock = clock(time);
	let day = local.date_naive();
	if day == today {
		cx.t_args("time-today", &[("time", ArgValue::from(clock))])
	} else if today.pred_opt() == Some(day) {
		cx.t_args("time-yesterday", &[("time", ArgValue::from(clock))])
	} else {
		format!("{:02}/{:02}/{}", day.month(), day.day(), day.year()).into()
	}
}

pub fn file_size(bytes: u64) -> String {
	let mut value = bytes as f64;
	let mut unit = 0;
	while value >= KIB && unit < 3 {
		value /= KIB;
		unit += 1;
	}
	let suffix = ["B", "KB", "MB", "GB"][unit];
	if unit == 0 {
		format!("{bytes} {suffix}")
	} else {
		format!("{value:.1} {suffix}")
	}
}

pub fn duration_label(seconds: f64) -> String {
	let total = seconds.max(0.) as u64;
	format!("{}:{:02}", total / 60, total % 60)
}

pub fn timestamp(unix: i64, style: TimestampStyle, cx: &App) -> String {
	let Some(time) = DateTime::from_timestamp(unix, 0) else {
		return unix.to_string();
	};
	let local = time.with_timezone(&Local);
	match style {
		TimestampStyle::ShortTime => local.format("%-I:%M %p").to_string(),
		TimestampStyle::LongTime => local.format("%-I:%M:%S %p").to_string(),
		TimestampStyle::ShortDate => local.format("%m/%d/%Y").to_string(),
		TimestampStyle::LongDate => local.format("%B %-d, %Y").to_string(),
		TimestampStyle::ShortDateTime => local.format("%B %-d, %Y %-I:%M %p").to_string(),
		TimestampStyle::LongDateTime => local.format("%A, %B %-d, %Y %-I:%M %p").to_string(),
		TimestampStyle::Relative => relative(time, cx).to_string(),
	}
}

fn relative(time: DateTime<Utc>, cx: &App) -> SharedString {
	let delta = Utc::now() - time;
	let past = delta >= TimeDelta::zero();
	let seconds = delta.num_seconds().unsigned_abs();
	let (key, amount) = match seconds {
		0..=59 => ("time-seconds", seconds),
		60..=3599 => ("time-minutes", seconds / 60),
		3600..=86_399 => ("time-hours", seconds / 3600),
		_ => ("time-days", seconds / 86_400),
	};
	let unit = cx.t_args(key, &[("count", ArgValue::from(amount as usize))]);
	let wrapper = if past { "time-ago" } else { "time-in" };
	cx.t_args(wrapper, &[("span", ArgValue::from(unit.to_string()))])
}
