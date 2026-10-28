use plugin_sdk::model::{ViewEvent, ViewEventKind};
use plugin_sdk::view;
use plugin_sdk::{HostError, Plugin, PluginExt, Reply, actions, data, export_plugin, host, log, mount};

const MOUNT_KEY: &str = "read-all";

#[derive(Default)]
struct ReadAllNotifications;

fn has_unread(guild_id: u64) -> bool {
	data::unread_channels(Some(guild_id)).is_ok_and(|channels| channels.iter().any(|state| state.unread))
}

fn read_everything() -> String {
	let guilds = match host::list_guilds() {
		Ok(guilds) => guilds,
		Err(error) => {
			log::warn!("could not list servers: {error:?}");
			return "Could not read servers".to_owned();
		}
	};
	let mut marked = 0;
	for guild in guilds.iter().filter(|guild| has_unread(guild.id)) {
		match actions::mark_guild_read(guild.id) {
			Ok(()) => marked += 1,
			Err(HostError::RateLimited) => return format!("Marked {marked}, click again for the rest"),
			Err(error) => log::warn!("could not mark {} read: {error:?}", guild.id),
		}
	}
	match marked {
		0 => "Nothing unread".to_owned(),
		1 => "Marked 1 server read".to_owned(),
		count => format!("Marked {count} servers read"),
	}
}

impl Plugin for ReadAllNotifications {
	fn init(&mut self) -> Result<(), String> {
		let button = view::icon_button(MOUNT_KEY, "check-check", "Mark all servers read");
		let options = mount::Options::new("Read all");
		if let Err(error) = mount::mount(mount::SERVER_RAIL_TOP, MOUNT_KEY, [button], options) {
			log::warn!("could not mount the button: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for ReadAllNotifications {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.node == MOUNT_KEY && event.kind == ViewEventKind::Click {
			return Reply::none().toast(read_everything());
		}
		Reply::none()
	}
}

export_plugin!(ReadAllNotifications, ext);
