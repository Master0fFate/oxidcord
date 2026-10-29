use plugin_sdk::model::{UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	HostError, MessageRef, Plugin, PluginExt, Reply, actions, client, export_plugin, log, mount, ui,
};

const BUTTON: &str = "download";

#[derive(Default)]
struct VoiceDownload;

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

fn is_audio(message: MessageRef, attachment_id: u64) -> bool {
	client::get_message(message.channel_id, message.id)
		.ok()
		.flatten()
		.is_some_and(|full| {
			full.attachments.iter().any(|attachment| {
				attachment.id == attachment_id
					&& attachment
						.content_type
						.as_deref()
						.is_some_and(|kind| kind.starts_with("audio/"))
			})
		})
}

impl Plugin for VoiceDownload {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = mount::provide(mount::ATTACHMENT_ACTION, mount::Options::new("Download")) {
			log::warn!("could not add the download button: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for VoiceDownload {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		if point != mount::ATTACHMENT_ACTION {
			return None;
		}
		let attachment_id = ui::context_detail()?.attachment_id?;
		is_audio(message_ref(context)?, attachment_id)
			.then(|| vec![view::icon_button(BUTTON, "download", "Download")])
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click
			|| event.point != mount::ATTACHMENT_ACTION
			|| event.node != BUTTON
		{
			return Reply::none();
		}
		let (Some(message), Some(attachment_id)) = (
			message_ref(&event.context),
			ui::context_detail().and_then(|detail| detail.attachment_id),
		) else {
			return Reply::none();
		};
		match actions::save_attachment(message, attachment_id) {
			Ok(()) => Reply::none(),
			Err(error) => Reply::none().toast(describe(&error)),
		}
	}
}

export_plugin!(VoiceDownload, ext);
