use plugin_sdk::model::{ViewEvent, ViewEventKind};
use plugin_sdk::view;
use plugin_sdk::{Plugin, PluginExt, Reply, client, export_plugin, log, mount};

const MOUNT_KEY: &str = "mention";
const BUTTON: &str = "mention";

#[derive(Default)]
struct QuickMention;

impl Plugin for QuickMention {
	fn init(&mut self) -> Result<(), String> {
		let button = view::icon_button(BUTTON, "at-sign", "Quick mention");
		let options = mount::Options::new("Quick mention");
		if let Err(error) = mount::mount(mount::MESSAGE_ACTION, MOUNT_KEY, [button], options) {
			log::warn!("could not add the message action: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for QuickMention {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.node != BUTTON || event.kind != ViewEventKind::Click {
			return Reply::none();
		}
		if let Some(author) = event.context.user_id
			&& let Err(error) = client::composer_insert(&format!("<@{author}> "))
		{
			log::warn!("could not insert the mention: {error:?}");
		}
		Reply::none()
	}
}

export_plugin!(QuickMention, ext);
