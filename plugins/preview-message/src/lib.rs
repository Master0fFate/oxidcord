use plugin_sdk::model::{Radius, Space, UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Color, Plugin, PluginExt, Reply, data, export_plugin, log, mount};

const TOOLBAR_KEY: &str = "preview";
const PREVIEW_BUTTON: &str = "preview";
const CLOSE_BUTTON: &str = "close";

#[derive(Default)]
struct PreviewMessage {
	shown: Option<(u64, String)>,
}

fn card(draft: &str) -> Node {
	view::column()
		.gap(Space::Sm)
		.padding(Space::Md)
		.background(Color::token("background-secondary"))
		.border(Color::token("divider"))
		.radius(Radius::Md)
		.child(
			view::row()
				.gap(Space::Sm)
				.child(view::icon("eye").icon_size(16))
				.child(view::text("Preview").bold())
				.child(view::spacer())
				.child(view::icon_button(CLOSE_BUTTON, "x", "Close")),
		)
		.child(view::markdown(draft))
}

impl Plugin for PreviewMessage {
	fn init(&mut self) -> Result<(), String> {
		let button = view::icon_button(PREVIEW_BUTTON, "eye", "Preview message");
		let options = mount::Options::new("Preview message");
		if let Err(error) = mount::mount(mount::COMPOSER_TOOLBAR, TOOLBAR_KEY, [button], options) {
			log::warn!("could not add the composer button: {error:?}");
		}
		if let Err(error) = mount::provide(mount::COMPOSER_ABOVE, mount::Options::new("Message preview")) {
			log::warn!("could not provide the preview: {error:?}");
		}
		Ok(())
	}
}

impl PreviewMessage {
	fn open(&mut self, channel_id: Option<u64>) -> Reply {
		let draft = data::composer_get_draft().unwrap_or_default();
		let Some(channel_id) = channel_id.filter(|_| !draft.trim().is_empty()) else {
			return Reply::none().toast("Nothing to preview");
		};
		self.shown = Some((channel_id, draft));
		self.invalidate();
		Reply::none()
	}

	fn invalidate(&self) {
		if let Err(error) = mount::invalidate(mount::COMPOSER_ABOVE) {
			log::debug!("could not refresh the preview: {error:?}");
		}
	}
}

impl PluginExt for PreviewMessage {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		let (channel_id, draft) = self.shown.as_ref()?;
		(point == mount::COMPOSER_ABOVE && context.channel_id == Some(*channel_id)).then(|| vec![card(draft)])
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if !matches!(event.kind, ViewEventKind::Click) {
			return Reply::none();
		}
		match event.node.as_str() {
			PREVIEW_BUTTON => self.open(event.context.channel_id),
			CLOSE_BUTTON => {
				self.shown = None;
				self.invalidate();
				Reply::none()
			}
			_ => Reply::none(),
		}
	}
}

export_plugin!(PreviewMessage, ext);
