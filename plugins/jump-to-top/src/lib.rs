use plugin_sdk::model::{UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Plugin, PluginExt, Reply, actions, export_plugin, log, mount};

const JUMP_BUTTON: &str = "jump";

#[derive(Default)]
struct JumpToTop;

impl Plugin for JumpToTop {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = mount::provide(mount::CHANNEL_HEADER, mount::Options::new("Jump to top")) {
			log::warn!("could not provide the channel header button: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for JumpToTop {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		(point == mount::CHANNEL_HEADER && context.channel_id.is_some())
			.then(|| vec![view::icon_button(JUMP_BUTTON, "arrow-up-to-line", "Jump to top")])
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.node == JUMP_BUTTON
			&& matches!(event.kind, ViewEventKind::Click)
			&& let Some(channel_id) = event.context.channel_id
			&& let Err(error) = actions::jump_to_top(channel_id)
		{
			log::debug!("could not jump to the top: {error:?}");
		}
		Reply::none()
	}
}

export_plugin!(JumpToTop, ext);
