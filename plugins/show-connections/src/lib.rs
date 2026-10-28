use plugin_sdk::model::{
	ButtonVariant, Connection, Space, TextSize, Tone, UiContext, ViewEvent, ViewEventKind,
};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, HostError, Plugin, PluginExt, Reply, data, export_plugin, host, log, mount, storage,
};

const ICON_SIZE_KEY: &str = "iconSize";
const ICON_SPACING_KEY: &str = "iconSpacing";
const LOAD_BUTTON: &str = "load";
const COPY_PREFIX: &str = "copy:";
const TITLE: &str = "Connections";
const MIN_ICON_SIZE: f64 = 16.;
const MAX_ICON_SIZE: f64 = 64.;
const DEFAULT_ICON_SIZE: u32 = 32;

struct Config {
	icon_size: u32,
	spacing: Space,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			icon_size: DEFAULT_ICON_SIZE,
			spacing: Space::Sm,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			ICON_SIZE_KEY => {
				self.icon_size = value.trim().parse::<f64>().map_or(DEFAULT_ICON_SIZE, |size| {
					size.clamp(MIN_ICON_SIZE, MAX_ICON_SIZE) as u32
				});
			}
			ICON_SPACING_KEY => {
				self.spacing = match value.trim() {
					"compact" => Space::Xs,
					"roomy" => Space::Md,
					_ => Space::Sm,
				};
			}
			_ => {}
		}
	}
}

#[derive(Default)]
struct ShowConnections {
	config: Config,
}

fn describe(error: &HostError) -> String {
	match error {
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Denied(_) | HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn icon_for(kind: &str) -> &'static str {
	match kind {
		"github" => "github",
		"spotify" | "amazon-music" => "music",
		"youtube" | "twitch" | "tiktok" => "video",
		"crunchyroll" => "tv",
		"steam" | "xbox" | "playstation" | "epicgames" | "battlenet" | "riotgames" | "leagueoflegends"
		| "roblox" | "bungie" => "gamepad-2",
		"twitter" | "facebook" | "instagram" | "reddit" | "bluesky" | "mastodon" | "skype" => {
			"message-circle"
		}
		"domain" => "globe",
		"paypal" => "credit-card",
		"ebay" => "shopping-cart",
		_ => "link",
	}
}

impl ShowConnections {
	fn row(&self, index: usize, connection: &Connection) -> Node {
		let name = match &connection.url {
			Some(url) => view::link(connection.name.clone(), url.clone()),
			None => view::text(connection.name.clone()),
		};
		let mut row = view::row()
			.gap(self.config.spacing)
			.child(
				view::icon(icon_for(&connection.kind))
					.icon_size(self.config.icon_size)
					.tooltip(connection.kind.clone()),
			)
			.child(name);
		if connection.verified {
			row = row.child(view::icon("badge-check").tone(Tone::Success).tooltip("Verified"));
		}
		if connection.url.is_none() {
			row = row.child(view::icon_button(
				format!("{COPY_PREFIX}{index}"),
				"copy",
				"Copy name",
			));
		}
		row
	}

	fn section(&self, connections: &[Connection]) -> Option<Node> {
		if connections.is_empty() {
			return None;
		}
		let rows = connections
			.iter()
			.enumerate()
			.map(|(index, connection)| self.row(index, connection));
		Some(view::section(TITLE).child(view::column().gap(self.config.spacing).children(rows)))
	}

	fn loaded(&self, connections: &[Connection]) -> Vec<Node> {
		vec![
			self.section(connections)
				.unwrap_or_else(|| view::section(TITLE).child(view::text("None").size(TextSize::Xs).muted())),
		]
	}
}

fn load_prompt() -> Vec<Node> {
	vec![
		view::section(TITLE).child(
			view::button(LOAD_BUTTON, "Load")
				.icon("download")
				.variant(ButtonVariant::Ghost),
		),
	]
}

impl Plugin for ShowConnections {
	fn init(&mut self) -> Result<(), String> {
		for key in [ICON_SIZE_KEY, ICON_SPACING_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		if let Err(error) = mount::provide(mount::USER_POPOUT_SECTION, mount::Options::new(TITLE)) {
			log::warn!("could not provide the popout section: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
			if let Err(error) = mount::invalidate(mount::USER_POPOUT_SECTION) {
				log::debug!("could not refresh the popout section: {error:?}");
			}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"iconSize":{"type":"integer","title":"Icon size","description":"Icon size in pixels","minimum":16,"maximum":64,"default":32},"iconSpacing":{"type":"string","title":"Icon margin","description":"Space between the icon and the name","enum":["compact","cozy","roomy"],"default":"cozy"}}}"#
			.to_owned()
	}
}

impl PluginExt for ShowConnections {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		if point != mount::USER_POPOUT_SECTION {
			return None;
		}
		match data::user_profile(context.user_id?) {
			Ok(Some(profile)) => self.section(&profile.connections).map(|section| vec![section]),
			Err(HostError::Unavailable) => Some(load_prompt()),
			_ => None,
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click || event.point != mount::USER_POPOUT_SECTION {
			return Reply::none();
		}
		let Some(user_id) = event.context.user_id else {
			return Reply::none();
		};
		let profile = match data::user_profile(user_id) {
			Ok(profile) => profile,
			Err(error) => return Reply::none().toast(describe(&error)),
		};
		let connections = profile.map(|profile| profile.connections).unwrap_or_default();
		if event.node == LOAD_BUTTON {
			return Reply::none().replace(self.loaded(&connections));
		}
		let name = event
			.node
			.strip_prefix(COPY_PREFIX)
			.and_then(|index| index.parse::<usize>().ok())
			.and_then(|index| connections.get(index))
			.map(|connection| connection.name.clone());
		match name.map(|name| host::clipboard_write(&name)) {
			Some(Ok(())) => Reply::none().toast("Copied"),
			Some(Err(error)) => Reply::none().toast(describe(&error)),
			None => Reply::none(),
		}
	}
}

export_plugin!(ShowConnections, ext);
