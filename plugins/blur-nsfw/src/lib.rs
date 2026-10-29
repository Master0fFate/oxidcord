use plugin_sdk::model::{AttachmentOverride, MessageFull, MessageMeta};
use plugin_sdk::patch::PatchExt;
use plugin_sdk::{Md, Plugin, PluginExt, export_plugin};

#[derive(Default)]
struct BlurNsfw;

fn blurred(id: u64) -> AttachmentOverride {
	AttachmentOverride {
		id,
		blur: true,
		alt: None,
		tooltip: None,
		url: None,
		hidden: false,
	}
}

impl Plugin for BlurNsfw {}

impl PluginExt for BlurNsfw {
	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		if !meta.nsfw_channel {
			return PatchExt::keep();
		}
		msg.attachments
			.iter()
			.fold(PatchExt::keep(), |patch, attachment| {
				patch.attachment(blurred(attachment.id))
			})
	}
}

export_plugin!(BlurNsfw, ext);
