use std::collections::{BTreeMap, BTreeSet};

use plugin_sdk::model::{AuthSession, ButtonVariant, Radius, Space, TextSize, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Color, Event, HostError, Plugin, PluginExt, Reply, data, export_plugin, host, log, mount, storage, timer,
};

const BACKGROUND_KEY: &str = "backgroundCheck";
const INTERVAL_KEY: &str = "checkInterval";
const NAMES_KEY: &str = "names";
const KNOWN_KEY: &str = "known";
const MOUNT_KEY: &str = "sessions";
const REFRESH: &str = "refresh";
const SAVE: &str = "save";
const NAME_FIELD: &str = "name";
const RENAME_PREFIX: &str = "rename-";
const START_TAG: &str = "start";
const RETRY_TAG: &str = "retry";
const TICK_TAG: &str = "tick";
const START_DELAY_MS: u32 = 3_000;
const RETRY_DELAY_MS: u32 = 65_000;
const MINUTE_MS: u32 = 60_000;
const DEFAULT_MINUTES: u32 = 20;
const MAX_MINUTES: u32 = 1_440;
const MAX_NAME_CHARS: usize = 48;
const MAX_ANNOUNCED: usize = 5;

struct Config {
	background: bool,
	minutes: u32,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			background: false,
			minutes: DEFAULT_MINUTES,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			BACKGROUND_KEY => self.background = matches!(value.trim(), "true" | "1" | "on"),
			INTERVAL_KEY => {
				if let Ok(minutes) = value.trim().parse::<f64>() {
					self.minutes = (minutes as u32).clamp(1, MAX_MINUTES);
				}
			}
			_ => {}
		}
	}
}

#[derive(Default)]
struct BetterSessions {
	config: Config,
	names: BTreeMap<String, String>,
	known: Option<BTreeSet<String>>,
	sessions: Vec<AuthSession>,
	ticker: Option<u64>,
}

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn summary(session: &AuthSession) -> String {
	format!("{} · {}", session.client_os, session.client_platform)
}

fn encode_names(names: &BTreeMap<String, String>) -> String {
	names.iter().map(|(id, name)| format!("{id}\t{name}\n")).collect()
}

fn decode_names(raw: &str) -> BTreeMap<String, String> {
	raw.lines()
		.filter_map(|line| line.split_once('\t'))
		.map(|(id, name)| (id.to_owned(), name.to_owned()))
		.collect()
}

fn announce(fresh: &[&AuthSession]) {
	if fresh.is_empty() {
		return;
	}
	let mut lines: Vec<String> = fresh
		.iter()
		.take(MAX_ANNOUNCED)
		.map(|session| format!("{} · {}", summary(session), session.location))
		.collect();
	if fresh.len() > MAX_ANNOUNCED {
		lines.push(format!("and {} more", fresh.len() - MAX_ANNOUNCED));
	}
	if let Err(error) = host::notify("New session", &lines.join("\n")) {
		log::debug!("notification skipped: {error:?}");
	}
}

impl BetterSessions {
	fn save_names(&self) {
		if let Err(error) = storage::set(NAMES_KEY, encode_names(&self.names).as_bytes()) {
			log::warn!("could not save the session names: {error:?}");
		}
	}

	fn save_known(&self, known: &BTreeSet<String>) {
		let raw: String = known.iter().map(|id| format!("{id}\n")).collect();
		if let Err(error) = storage::set(KNOWN_KEY, raw.as_bytes()) {
			log::warn!("could not save the session list: {error:?}");
		}
	}

	fn check(&mut self) -> Result<(), HostError> {
		let mut sessions = data::list_sessions()?;
		if sessions.is_empty() {
			return Err(HostError::Unavailable);
		}
		sessions.sort_by_key(|session| std::cmp::Reverse(session.approx_last_used_ms));
		let current: BTreeSet<String> = sessions.iter().map(|session| session.id.clone()).collect();
		if let Some(known) = &self.known {
			let fresh: Vec<&AuthSession> = sessions
				.iter()
				.filter(|session| !known.contains(&session.id))
				.collect();
			announce(&fresh);
		}
		if self.known.as_ref() != Some(&current) {
			self.save_known(&current);
		}
		let before = self.names.len();
		self.names.retain(|id, _| current.contains(id));
		if self.names.len() != before {
			self.save_names();
		}
		self.known = Some(current);
		self.sessions = sessions;
		Ok(())
	}

	fn sync_ticker(&mut self) {
		if let Some(id) = self.ticker.take() {
			timer::clear(id);
		}
		if !self.config.background {
			return;
		}
		match timer::every(self.config.minutes * MINUTE_MS, TICK_TAG) {
			Ok(id) => self.ticker = Some(id),
			Err(error) => log::warn!("could not schedule the background check: {error:?}"),
		}
	}

	fn refresh_page(&self) {
		if let Err(error) = mount::update(mount::SETTINGS_PAGE, MOUNT_KEY, self.page()) {
			log::debug!("could not update the sessions page: {error:?}");
		}
	}

	fn card(&self, index: usize, session: &AuthSession) -> Node {
		let custom = self.names.get(&session.id);
		let title = custom.map_or_else(|| summary(session), |name| format!("{name}*"));
		let mut heading = view::row().gap(Space::Sm).child(view::text(title).bold());
		if session.current {
			heading = heading.child(view::badge("This device"));
		}
		let detail = view::row()
			.gap(Space::Sm)
			.wrap()
			.child(view::text(session.location.clone()).size(TextSize::Xs).muted())
			.child(view::timestamp(session.approx_last_used_ms, "F"))
			.child(view::timestamp(session.approx_last_used_ms, "R"));
		let rename = view::form().key(format!("{RENAME_PREFIX}{index}")).child(
			view::row()
				.gap(Space::Sm)
				.child(view::input(NAME_FIELD, "Custom name").value(custom.cloned().unwrap_or_default()))
				.child(
					view::button(SAVE, "Save")
						.variant(ButtonVariant::Outline)
						.submit(),
				),
		);
		view::column()
			.gap(Space::Sm)
			.padding(Space::Md)
			.background(Color::token("background-secondary"))
			.radius(Radius::Md)
			.child(heading)
			.child(detail)
			.child(rename)
	}

	fn page(&self) -> Vec<Node> {
		let mut column = view::column().gap(Space::Md).padding(Space::Md).child(
			view::row()
				.gap(Space::Md)
				.child(view::heading("Sessions"))
				.child(view::spacer())
				.child(
					view::button(REFRESH, "Refresh")
						.icon("rotate-cw")
						.variant(ButtonVariant::Outline),
				),
		);
		if self.sessions.is_empty() {
			column = column.child(view::text("No sessions loaded yet, press Refresh in a moment").muted());
		}
		for (index, session) in self.sessions.iter().enumerate() {
			column = column.child(self.card(index, session));
		}
		vec![column]
	}

	fn rename(&mut self, index: usize, values: &[(String, String)]) -> Reply {
		let Some(id) = self.sessions.get(index).map(|session| session.id.clone()) else {
			return Reply::none();
		};
		let name: String = values
			.iter()
			.find(|(key, _)| key == NAME_FIELD)
			.map(|(_, value)| value.trim().chars().take(MAX_NAME_CHARS).collect())
			.unwrap_or_default();
		if name.is_empty() || name.contains(['\t', '\n', '\r']) {
			self.names.remove(&id);
		} else {
			self.names.insert(id, name);
		}
		self.save_names();
		Reply::none().replace(self.page()).toast("Saved")
	}
}

impl Plugin for BetterSessions {
	fn init(&mut self) -> Result<(), String> {
		for key in [BACKGROUND_KEY, INTERVAL_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		if let Ok(Some(saved)) = storage::get(NAMES_KEY) {
			self.names = decode_names(&String::from_utf8_lossy(&saved));
		}
		if let Ok(Some(saved)) = storage::get(KNOWN_KEY) {
			let raw = String::from_utf8_lossy(&saved);
			self.known = Some(raw.lines().map(str::to_owned).collect());
		}
		let mounted = mount::mount(
			mount::SETTINGS_PAGE,
			MOUNT_KEY,
			self.page(),
			mount::Options::new("Sessions"),
		);
		if let Err(error) = mounted {
			log::warn!("could not mount the sessions page: {error:?}");
		}
		if let Err(error) = timer::once(START_DELAY_MS, START_TAG) {
			log::warn!("could not schedule the startup check: {error:?}");
		}
		self.sync_ticker();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
			if key == BACKGROUND_KEY || key == INTERVAL_KEY {
				self.sync_ticker();
			}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"backgroundCheck":{"type":"boolean","title":"Background check","description":"Check for new sessions in the background and notify when one appears","default":false},"checkInterval":{"type":"number","title":"Check interval","description":"How often to check for new sessions in the background, in minutes","minimum":1,"maximum":1440,"default":20}}}"#
			.to_owned()
	}
}

impl PluginExt for BetterSessions {
	fn on_timer(&mut self, _id: u64, tag: &str) {
		if !matches!(tag, START_TAG | RETRY_TAG | TICK_TAG) {
			return;
		}
		match self.check() {
			Ok(()) => self.refresh_page(),
			Err(HostError::Unavailable) if tag == START_TAG => {
				if let Err(error) = timer::once(RETRY_DELAY_MS, RETRY_TAG) {
					log::warn!("could not schedule the retry: {error:?}");
				}
			}
			Err(error) => log::debug!("session check skipped: {error:?}"),
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.point != mount::SETTINGS_PAGE {
			return Reply::none();
		}
		match (&event.kind, event.node.as_str()) {
			(ViewEventKind::Click, REFRESH) => match self.check() {
				Ok(()) => Reply::none().replace(self.page()),
				Err(error) => Reply::none().toast(describe(&error)),
			},
			(ViewEventKind::Submit(values), node) => node
				.strip_prefix(RENAME_PREFIX)
				.and_then(|index| index.parse().ok())
				.map_or_else(Reply::none, |index| self.rename(index, values)),
			_ => Reply::none(),
		}
	}
}

export_plugin!(BetterSessions, ext);
