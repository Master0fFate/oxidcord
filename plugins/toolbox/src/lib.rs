use plugin_sdk::model::{ButtonVariant, Length, PluginSummary, Space, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, Plugin, PluginExt, Reply, actions, data, export_plugin, host, log, mount, storage, timer,
};

const SHOW_PLUGINS_KEY: &str = "showPluginMenu";
const MOUNT_KEY: &str = "toolbox";
const TRIGGER: &str = "trigger";
const OPEN_PLUGINS: &str = "open-plugins";
const COPY_VERSION: &str = "copy-version";
const REFRESH: &str = "refresh";
const SETTLE_TAG: &str = "settle";
const SETTLE_MS: u32 = 3_000;
const LIST_HEIGHT: u32 = 240;
const MAX_LISTED: usize = 100;
const PLUGINS_PAGE: &str = "plugins";

struct Toolbox {
	show_plugins: bool,
}

impl Default for Toolbox {
	fn default() -> Self {
		Self { show_plugins: true }
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

fn plugin_row(plugin: &PluginSummary) -> Node {
	let row = view::row()
		.gap(Space::Sm)
		.child(view::text(plugin.name.clone()))
		.child(view::spacer())
		.child(view::text(plugin.version.clone()).muted());
	if plugin.ext {
		row.child(view::badge("ext"))
	} else {
		row
	}
}

fn plugin_list() -> Node {
	let mut enabled: Vec<PluginSummary> = data::list_plugins()
		.unwrap_or_default()
		.into_iter()
		.filter(|plugin| plugin.enabled)
		.collect();
	enabled.sort_by_cached_key(|plugin| plugin.name.to_lowercase());
	let hidden = enabled.len().saturating_sub(MAX_LISTED);
	let mut list = view::column().gap(Space::Xs);
	for plugin in enabled.iter().take(MAX_LISTED) {
		list = list.child(plugin_row(plugin));
	}
	if hidden > 0 {
		list = list.child(view::text(format!("and {hidden} more")).muted());
	}
	view::scroll().height(Length::Px(LIST_HEIGHT)).child(list)
}

impl Toolbox {
	fn popover(&self) -> Node {
		let mut popover = view::popover(view::icon_button(TRIGGER, "wrench", "Toolbox"))
			.child(view::heading("Toolbox"))
			.child(
				view::button(OPEN_PLUGINS, "Plugin settings")
					.icon("settings")
					.variant(ButtonVariant::Outline),
			)
			.child(
				view::button(COPY_VERSION, "Copy version")
					.icon("copy")
					.variant(ButtonVariant::Outline),
			);
		if self.show_plugins {
			popover = popover
				.child(view::divider())
				.child(
					view::row()
						.child(view::text("Plugins").bold())
						.child(view::spacer())
						.child(view::icon_button(REFRESH, "rotate-cw", "Refresh")),
				)
				.child(plugin_list());
		}
		popover
	}

	fn refresh(&self) {
		if let Err(error) = mount::update(mount::TITLE_BAR, MOUNT_KEY, [self.popover()]) {
			log::debug!("could not refresh the toolbox: {error:?}");
		}
	}
}

fn copy_version() -> String {
	let info = match data::client_info() {
		Ok(info) => info,
		Err(error) => {
			log::debug!("could not read the client version: {error:?}");
			return "Version not available".to_owned();
		}
	};
	match host::clipboard_write(&format!("Oxidcord {} ({})", info.version, info.platform)) {
		Ok(()) => "Version copied".to_owned(),
		Err(error) => {
			log::debug!("could not copy the version: {error:?}");
			"Could not copy".to_owned()
		}
	}
}

impl Plugin for Toolbox {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{SHOW_PLUGINS_KEY}")) {
			self.show_plugins = parse_flag(&String::from_utf8_lossy(&saved));
		}
		let mounted = mount::mount(
			mount::TITLE_BAR,
			MOUNT_KEY,
			[self.popover()],
			mount::Options::new("Toolbox"),
		);
		if let Err(error) = mounted {
			log::warn!("could not mount the toolbox: {error:?}");
		}
		if let Err(error) = timer::once(SETTLE_MS, SETTLE_TAG) {
			log::warn!("could not schedule the plugin list refresh: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == SHOW_PLUGINS_KEY
		{
			self.show_plugins = parse_flag(&value);
			self.refresh();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"showPluginMenu":{"type":"boolean","title":"Show plugin list","description":"Show the plugins menu in the toolbox","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for Toolbox {
	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == SETTLE_TAG && self.show_plugins {
			self.refresh();
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click {
			return Reply::none();
		}
		match event.node.as_str() {
			OPEN_PLUGINS => {
				if let Err(error) = actions::open_settings(PLUGINS_PAGE) {
					log::debug!("could not open the plugin settings: {error:?}");
				}
				Reply::none().close()
			}
			COPY_VERSION => Reply::none().toast(copy_version()),
			REFRESH => Reply::none().replace([self.popover()]),
			_ => Reply::none(),
		}
	}
}

export_plugin!(Toolbox, ext);
