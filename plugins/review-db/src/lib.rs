use std::collections::HashMap;

use plugin_sdk::model::{ButtonVariant, Space, TextSize, UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::types::{FetchRequest, SettingChange};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Event, Plugin, PluginExt, Reply, export_plugin, host, log, mount, now_ms, storage};
use serde_json::Value;

const API_URL: &str = "https://manti.vendicated.dev/api/reviewdb/users";
const SHOW_WARNING_KEY: &str = "showWarning";
const HIDE_TIMESTAMPS_KEY: &str = "hideTimestamps";
const REFRESH_BUTTON: &str = "refresh";
const SYSTEM_TYPE: i64 = 3;
const SHOWN_REVIEWS: usize = 10;
const KEPT_COMMENT_CHARS: usize = 500;
const LOADED_MS: i64 = 10 * 60 * 1000;
const FAILED_MS: i64 = 60 * 1000;
const CACHE_CAPACITY: usize = 64;

struct Review {
	author: String,
	comment: String,
	timestamp: i64,
	system: bool,
}

enum Entry {
	Loaded { reviews: Vec<Review>, total: u64 },
	Failed(String),
}

struct Cached {
	at_ms: i64,
	entry: Entry,
}

impl Cached {
	fn is_fresh(&self) -> bool {
		let lifetime = match self.entry {
			Entry::Loaded { .. } => LOADED_MS,
			Entry::Failed(_) => FAILED_MS,
		};
		now_ms() - self.at_ms < lifetime
	}
}

struct ReviewDb {
	show_warning: bool,
	hide_timestamps: bool,
	cache: HashMap<u64, Cached>,
}

impl Default for ReviewDb {
	fn default() -> Self {
		Self {
			show_warning: true,
			hide_timestamps: false,
			cache: HashMap::new(),
		}
	}
}

fn parse_flag(raw: &str, default: bool) -> bool {
	match raw.trim() {
		"true" | "1" | "on" => true,
		"false" | "0" | "off" => false,
		_ => default,
	}
}

fn parse_review(value: &Value) -> Option<Review> {
	Some(Review {
		author: value.get("sender")?.get("username")?.as_str()?.to_owned(),
		comment: value
			.get("comment")?
			.as_str()?
			.chars()
			.take(KEPT_COMMENT_CHARS)
			.collect(),
		timestamp: value.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
		system: value.get("type").and_then(Value::as_i64) == Some(SYSTEM_TYPE),
	})
}

fn fetch_reviews(user_id: u64) -> Entry {
	let request = FetchRequest {
		method: "GET".to_owned(),
		url: format!("{API_URL}/{user_id}/reviews?limit={SHOWN_REVIEWS}"),
		headers: vec![("accept".to_owned(), "application/json".to_owned())],
		body: None,
	};
	let response = match host::fetch(&request) {
		Ok(response) => response,
		Err(error) => {
			log::warn!("could not reach ReviewDB: {error:?}");
			return Entry::Failed("Could not reach ReviewDB".to_owned());
		}
	};
	match response.status {
		200 => {}
		429 => return Entry::Failed("ReviewDB is busy, try again in a moment".to_owned()),
		_ => return Entry::Failed("ReviewDB could not load the reviews".to_owned()),
	}
	let Ok(answer) = serde_json::from_slice::<Value>(&response.body) else {
		return Entry::Failed("ReviewDB sent an unreadable answer".to_owned());
	};
	let reviews = answer
		.get("reviews")
		.and_then(Value::as_array)
		.map(|items| items.iter().filter_map(parse_review).collect())
		.unwrap_or_default();
	Entry::Loaded {
		reviews,
		total: answer.get("reviewCount").and_then(Value::as_u64).unwrap_or(0),
	}
}

fn review_row(review: &Review, hide_timestamps: bool) -> Node {
	let mut header = view::row()
		.gap(Space::Sm)
		.child(view::text(review.author.clone()).bold());
	if !hide_timestamps && review.timestamp > 0 {
		header = header.child(view::timestamp(review.timestamp * 1000, "R"));
	}
	view::column()
		.gap(Space::Xs)
		.fill()
		.child(header)
		.child(view::text(review.comment.clone()))
}

fn section(cached: &Cached, show_warning: bool, hide_timestamps: bool) -> Node {
	let (title, body) = match &cached.entry {
		Entry::Loaded { reviews, total } => {
			let rows: Vec<Node> = reviews
				.iter()
				.filter(|review| show_warning || !review.system)
				.map(|review| review_row(review, hide_timestamps))
				.collect();
			let body = if rows.is_empty() {
				view::text("No reviews yet").muted()
			} else {
				view::column().gap(Space::Md).fill().children(rows)
			};
			(format!("Reviews ({total})"), body)
		}
		Entry::Failed(message) => ("Reviews".to_owned(), view::text(message.clone()).muted()),
	};
	view::section(title)
		.collapsed(true)
		.child(body)
		.child(
			view::button(REFRESH_BUTTON, "Refresh")
				.icon("refresh-cw")
				.variant(ButtonVariant::Ghost),
		)
		.child(
			view::text("Reviews come from ReviewDB, read only")
				.size(TextSize::Tiny)
				.muted(),
		)
}

impl ReviewDb {
	fn view(&mut self, user_id: u64, force: bool) -> Node {
		if force || !self.cache.get(&user_id).is_some_and(Cached::is_fresh) {
			if self.cache.len() >= CACHE_CAPACITY {
				self.cache.clear();
			}
			let entry = fetch_reviews(user_id);
			self.cache.insert(
				user_id,
				Cached {
					at_ms: now_ms(),
					entry,
				},
			);
		}
		match self.cache.get(&user_id) {
			Some(cached) => section(cached, self.show_warning, self.hide_timestamps),
			None => view::text("No reviews").muted(),
		}
	}
}

impl Plugin for ReviewDb {
	fn init(&mut self) -> Result<(), String> {
		let read = |key: &str, default: bool| {
			storage::get(&format!("settings.{key}"))
				.ok()
				.flatten()
				.map_or(default, |bytes| {
					parse_flag(&String::from_utf8_lossy(&bytes), default)
				})
		};
		self.show_warning = read(SHOW_WARNING_KEY, true);
		self.hide_timestamps = read(HIDE_TIMESTAMPS_KEY, false);
		if let Err(error) = mount::provide(mount::USER_POPOUT_SECTION, mount::Options::new("Reviews")) {
			log::warn!("could not add the reviews section: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		let Event::SettingChanged(SettingChange { key, value }) = event else {
			return;
		};
		match key.as_str() {
			SHOW_WARNING_KEY => self.show_warning = parse_flag(&value, true),
			HIDE_TIMESTAMPS_KEY => self.hide_timestamps = parse_flag(&value, false),
			_ => return,
		}
		if let Err(error) = mount::invalidate(mount::USER_POPOUT_SECTION) {
			log::warn!("could not refresh the reviews section: {error:?}");
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"showWarning":{"type":"boolean","title":"Show warning","description":"Display the reminder to be respectful at the top of the reviews","default":true},"hideTimestamps":{"type":"boolean","title":"Hide timestamps","description":"Hide the time on reviews","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for ReviewDb {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		if point != mount::USER_POPOUT_SECTION {
			return None;
		}
		Some(vec![self.view(context.user_id?, false)])
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		match (event.context.user_id, &event.kind, event.node.as_str()) {
			(Some(user_id), ViewEventKind::Click, REFRESH_BUTTON) => {
				Reply::none().replace([self.view(user_id, true)])
			}
			_ => Reply::none(),
		}
	}
}

export_plugin!(ReviewDb, ext);
