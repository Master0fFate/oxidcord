use plugin_sdk::model::{Align, ClientEvent, RelationshipKind, Space, TextSize};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Event, Plugin, PluginExt, data, export_plugin, host, log, mount, storage, timer};

const MOUNT_KEY: &str = "indicators";
const REFRESH_TAG: &str = "refresh";
const REFRESH_MS: u32 = 30_000;
const DEBOUNCE_MS: u32 = 2_000;
const MODE_KEY: &str = "mode";

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
	#[default]
	Friends,
	Servers,
	Both,
}

impl Mode {
	fn parse(raw: &str) -> Self {
		match raw.trim() {
			"servers" => Self::Servers,
			"both" => Self::Both,
			_ => Self::Friends,
		}
	}

	fn shows_friends(self) -> bool {
		self != Self::Servers
	}

	fn shows_servers(self) -> bool {
		self != Self::Friends
	}
}

#[derive(Default)]
struct ServerListIndicators {
	mode: Mode,
	refresh_queued: bool,
}

fn online_friends() -> usize {
	let friends = match data::list_relationships() {
		Ok(people) => people,
		Err(error) => {
			log::warn!("could not read friends: {error:?}");
			return 0;
		}
	};
	friends
		.iter()
		.filter(|relationship| relationship.kind == RelationshipKind::Friend)
		.filter(|relationship| {
			data::user_presence(relationship.user.id)
				.ok()
				.flatten()
				.is_some_and(|presence| !matches!(presence.status.as_str(), "offline" | "invisible"))
		})
		.count()
}

fn server_count() -> usize {
	match host::list_guilds() {
		Ok(guilds) => guilds.len(),
		Err(error) => {
			log::warn!("could not read servers: {error:?}");
			0
		}
	}
}

fn label(value: String) -> Node {
	view::text(value).size(TextSize::Xs).bold()
}

impl ServerListIndicators {
	fn page(&self) -> Vec<Node> {
		let mut column = view::column().align(Align::Center).gap(Space::Xs);
		if self.mode.shows_friends() {
			column = column.child(label(format!("{} online", online_friends())));
		}
		if self.mode.shows_servers() {
			column = column.child(label(format!("{} servers", server_count())));
		}
		vec![column]
	}

	fn refresh(&mut self) {
		self.refresh_queued = false;
		if let Err(error) = mount::update(mount::SERVER_RAIL_TOP, MOUNT_KEY, self.page()) {
			log::warn!("could not update the indicators: {error:?}");
		}
	}

	fn queue_refresh(&mut self) {
		if self.refresh_queued {
			return;
		}
		match timer::once(DEBOUNCE_MS, REFRESH_TAG) {
			Ok(_) => self.refresh_queued = true,
			Err(error) => log::warn!("could not schedule a refresh: {error:?}"),
		}
	}
}

impl Plugin for ServerListIndicators {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{MODE_KEY}")) {
			self.mode = Mode::parse(&String::from_utf8_lossy(&saved));
		}
		let options = mount::Options::new("Indicators");
		if let Err(error) = mount::mount(mount::SERVER_RAIL_TOP, MOUNT_KEY, self.page(), options) {
			log::warn!("could not mount the indicators: {error:?}");
		}
		if let Err(error) = timer::every(REFRESH_MS, REFRESH_TAG) {
			log::warn!("could not start the refresh timer: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == MODE_KEY
		{
			self.mode = Mode::parse(&value);
			self.queue_refresh();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"mode":{"type":"string","title":"Mode","description":"friends shows the online friend count, servers the server count, both shows the two","enum":["friends","servers","both"],"default":"friends"}}}"#
			.to_owned()
	}
}

impl PluginExt for ServerListIndicators {
	fn on_client_event(&mut self, event: ClientEvent) {
		if matches!(
			event,
			ClientEvent::PresenceChanged(_)
				| ClientEvent::RelationshipChanged(_)
				| ClientEvent::RelationshipRemoved(_)
				| ClientEvent::GuildJoined(_)
				| ClientEvent::GuildLeft(_)
				| ClientEvent::Ready
		) {
			self.queue_refresh();
		}
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == REFRESH_TAG {
			self.refresh();
		}
	}
}

export_plugin!(ServerListIndicators, ext);
