use plugin_sdk::model::{ButtonVariant, Space, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Plugin, PluginExt, Reply, data, export_plugin, log, mount, timer};

const MOUNT_KEY: &str = "timings";
const REFRESH: &str = "refresh";
const SETTLE_TAG: &str = "settle";
const SETTLE_MS: u32 = 5_000;

#[derive(Default)]
struct StartupTimings;

fn duration(ms: i64) -> String {
	let seconds = ms / 1000;
	match seconds {
		0..60 => format!("{:.1} s", ms as f64 / 1000.),
		60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
		_ => format!("{}h {}m {}s", seconds / 3600, seconds % 3600 / 60, seconds % 60),
	}
}

fn fact(label: impl Into<String>, value: impl Into<String>) -> Node {
	view::row()
		.gap(Space::Md)
		.child(view::text(label))
		.child(view::spacer())
		.child(view::text(value).mono())
}

fn page() -> Vec<Node> {
	let mut column = view::column().gap(Space::Sm).padding(Space::Md).child(
		view::row()
			.gap(Space::Md)
			.child(view::heading("Startup timings"))
			.child(view::spacer())
			.child(
				view::button(REFRESH, "Refresh")
					.icon("rotate-cw")
					.variant(ButtonVariant::Outline),
			),
	);
	column = match data::client_info() {
		Ok(info) => info.startup.iter().fold(
			column
				.child(fact("Uptime", duration(info.uptime_ms)))
				.child(view::divider()),
			|column, (phase, offset)| column.child(fact(phase.clone(), format!("+{offset} ms"))),
		),
		Err(error) => {
			log::debug!("could not read the startup timings: {error:?}");
			column.child(view::text("Not available right now").muted())
		}
	};
	vec![column]
}

impl Plugin for StartupTimings {
	fn init(&mut self) -> Result<(), String> {
		let options = mount::Options::new("Startup timings").icon("clock");
		if let Err(error) = mount::mount(mount::SETTINGS_PAGE, MOUNT_KEY, page(), options) {
			log::warn!("could not mount the startup timings page: {error:?}");
		}
		if let Err(error) = timer::once(SETTLE_MS, SETTLE_TAG) {
			log::warn!("could not schedule the page refresh: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for StartupTimings {
	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == SETTLE_TAG
			&& let Err(error) = mount::update(mount::SETTINGS_PAGE, MOUNT_KEY, page())
		{
			log::debug!("could not refresh the startup timings page: {error:?}");
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.node == REFRESH && event.kind == ViewEventKind::Click {
			return Reply::none().replace(page());
		}
		Reply::none()
	}
}

export_plugin!(StartupTimings, ext);
