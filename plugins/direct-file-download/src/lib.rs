use plugin_sdk::model::{UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	HostError, MessageRef, Plugin, PluginExt, Reply, actions, export_plugin, log, menu, mount, ui,
};

const BUTTON: &str = "download";
const MENU_ITEM: &str = "save-as";
const TOAST_MS: u32 = 3000;

#[derive(Default)]
struct DirectFileDownload;

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn message_ref(context: &UiContext) -> Option<MessageRef> {
	Some(MessageRef {
		id: context.message_id?,
		channel_id: context.channel_id?,
		guild_id: context.guild_id,
	})
}

fn save(context: &UiContext) -> Result<(), HostError> {
	match (
		message_ref(context),
		ui::context_detail().and_then(|detail| detail.attachment_id),
	) {
		(Some(message), Some(attachment_id)) => actions::save_attachment(message, attachment_id),
		_ => Ok(()),
	}
}

impl Plugin for DirectFileDownload {
	fn init(&mut self) -> Result<(), String> {
		let added =
			mount::provide(mount::ATTACHMENT_ACTION, mount::Options::new("Download")).and_then(|()| {
				menu::set(
					menu::ATTACHMENT,
					[menu::item(MENU_ITEM, "Save as").icon("download")],
				)
			});
		if let Err(error) = added {
			log::warn!("could not add the download controls: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for DirectFileDownload {
	fn render_view(&mut self, point: &str, _context: &UiContext) -> Option<Vec<Node>> {
		(point == mount::ATTACHMENT_ACTION).then(|| vec![view::icon_button(BUTTON, "download", "Download")])
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click
			|| event.point != mount::ATTACHMENT_ACTION
			|| event.node != BUTTON
		{
			return Reply::none();
		}
		match save(&event.context) {
			Ok(()) => Reply::none(),
			Err(error) => Reply::none().toast(describe(&error)),
		}
	}

	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::ATTACHMENT || key != MENU_ITEM {
			return;
		}
		if let Err(error) = save(context)
			&& let Err(error) = mount::toast([view::text(describe(&error))], TOAST_MS)
		{
			log::warn!("could not show the toast: {error:?}");
		}
	}
}

export_plugin!(DirectFileDownload, ext);
