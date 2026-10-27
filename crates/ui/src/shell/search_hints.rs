use chrono::{Duration, Local};
use gpui::SharedString;
use protocol::{ChannelType, Snowflake};
use state::State;

use super::search_panel::{AUTHOR_TYPES, Filter, HAS_VALUES};
use crate::icons::{IconKind, channel_icon};
use crate::lookup::avatar_for;

const SUGGESTION_LIMIT: usize = 8;

pub enum Leading {
	Icon(IconKind),
	Avatar(String),
}

pub struct Suggestion {
	pub leading: Leading,
	pub title: SharedString,
	pub detail: Option<SharedString>,
	pub value: String,
}

pub struct Pending<'a> {
	pub filter: Filter,
	pub partial: &'a str,
	pub start: usize,
}

pub fn pending(text: &str) -> Option<Pending<'_>> {
	if text.ends_with(char::is_whitespace) {
		return None;
	}
	let start = text.rfind(char::is_whitespace).map_or(0, |at| at + 1);
	let (key, partial) = text[start..].split_once(':')?;
	Some(Pending {
		filter: Filter::named(key)?,
		partial,
		start,
	})
}

pub fn complete(text: &str, pending: &Pending, value: &str) -> String {
	format!("{}{}:{value} ", &text[..pending.start], pending.filter.key())
}

pub fn with_filter(text: &str, filter: Filter) -> String {
	let trimmed = text.trim_end();
	if trimmed.is_empty() {
		format!("{}:", filter.key())
	} else {
		format!("{trimmed} {}:", filter.key())
	}
}

pub fn suggestions(
	pending: &Pending,
	state: &State,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
	labels: &DateLabels,
) -> Vec<Suggestion> {
	let needle = pending.partial.to_lowercase();
	match pending.filter {
		Filter::From | Filter::Mentions => users(state, guild, channel, &needle),
		Filter::In => channels(state, guild, &needle),
		Filter::Has => values(&HAS_VALUES, &needle, IconKind::Attach),
		Filter::AuthorType => values(&AUTHOR_TYPES, &needle, IconKind::Members),
		Filter::Pinned => values(&["true", "false"], &needle, IconKind::Pin),
		Filter::Before | Filter::During | Filter::After => dates(labels),
	}
}

pub struct DateLabels {
	pub today: SharedString,
	pub yesterday: SharedString,
}

fn dates(labels: &DateLabels) -> Vec<Suggestion> {
	let today = Local::now().date_naive();
	[
		(labels.today.clone(), today),
		(labels.yesterday.clone(), today - Duration::days(1)),
	]
	.into_iter()
	.map(|(title, date)| {
		let value = date.format("%Y-%m-%d").to_string();
		Suggestion {
			leading: Leading::Icon(IconKind::Calendar),
			title,
			detail: Some(value.clone().into()),
			value,
		}
	})
	.collect()
}

fn values(options: &[&str], needle: &str, icon: IconKind) -> Vec<Suggestion> {
	options
		.iter()
		.filter(|option| option.starts_with(needle))
		.map(|option| Suggestion {
			leading: Leading::Icon(icon),
			title: (*option).to_owned().into(),
			detail: None,
			value: (*option).to_owned(),
		})
		.collect()
}

fn users(
	state: &State,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
	needle: &str,
) -> Vec<Suggestion> {
	let candidates: Vec<Snowflake> = match guild.and_then(|id| state.guild(id)) {
		Some(found) => found.members.keys().copied().collect(),
		None => state
			.dm_rows()
			.into_iter()
			.filter(|row| channel.is_none_or(|channel| row.channel_id == channel))
			.flat_map(|row| row.recipients)
			.chain(state.current_user_id())
			.collect(),
	};
	let mut ranked: Vec<(u8, String, Snowflake, String)> = candidates
		.into_iter()
		.filter_map(|id| {
			let username = state.user(id)?.username.clone();
			let display = state.display_name(guild, id);
			let names = [username.to_lowercase(), display.to_lowercase()];
			let rank = if names.iter().any(|name| name.starts_with(needle)) {
				0
			} else if names.iter().any(|name| name.contains(needle)) {
				1
			} else {
				return None;
			};
			Some((rank, display, id, username))
		})
		.collect();
	ranked.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
	ranked
		.into_iter()
		.take(SUGGESTION_LIMIT)
		.map(|(_, display, id, username)| Suggestion {
			leading: Leading::Avatar(avatar_for(state, id, 64)),
			title: display.into(),
			detail: Some(username.clone().into()),
			value: username,
		})
		.collect()
}

fn channels(state: &State, guild: Option<Snowflake>, needle: &str) -> Vec<Suggestion> {
	let Some(guild) = guild else {
		return Vec::new();
	};
	let mut matches: Vec<(bool, String, ChannelType)> = state
		.guild_channel_tree(guild)
		.into_iter()
		.flat_map(|node| node.children)
		.filter(|channel| !channel.locked)
		.filter_map(|channel| {
			let lowered = channel.name.to_lowercase();
			lowered
				.contains(needle)
				.then(|| (!lowered.starts_with(needle), channel.name, channel.kind))
		})
		.collect();
	matches.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
	matches
		.into_iter()
		.take(SUGGESTION_LIMIT)
		.map(|(_, name, kind)| Suggestion {
			leading: Leading::Icon(channel_icon(kind)),
			title: name.clone().into(),
			detail: None,
			value: name,
		})
		.collect()
}
