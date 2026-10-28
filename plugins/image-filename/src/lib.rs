use plugin_sdk::model::{AttachmentInfo, AttachmentOverride, MessageFull, MessageMeta};
use plugin_sdk::patch::PatchExt;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, Plugin, PluginExt, export_plugin, storage};

const SHOW_FULL_URL_KEY: &str = "showFullUrl";
const IMAGE_EXTENSIONS: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif"];

#[derive(Default)]
struct ImageFilename {
	show_full_url: bool,
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

fn is_image(attachment: &AttachmentInfo) -> bool {
	if attachment
		.content_type
		.as_deref()
		.is_some_and(|kind| kind.starts_with("image/"))
	{
		return true;
	}
	let name = attachment.filename.to_ascii_lowercase();
	IMAGE_EXTENSIONS.iter().any(|extension| name.ends_with(extension))
}

impl ImageFilename {
	fn tooltip(&self, attachment: &AttachmentInfo) -> AttachmentOverride {
		let text = if self.show_full_url {
			attachment.url.clone()
		} else {
			attachment.filename.clone()
		};
		AttachmentOverride {
			id: attachment.id,
			blur: false,
			alt: None,
			tooltip: Some(text),
			url: None,
			hidden: false,
		}
	}
}

impl Plugin for ImageFilename {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{SHOW_FULL_URL_KEY}")) {
			self.show_full_url = parse_flag(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == SHOW_FULL_URL_KEY
		{
			self.show_full_url = parse_flag(&value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"showFullUrl":{"type":"boolean","title":"Show full URL","description":"Show the full URL of the image instead of just the file name","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for ImageFilename {
	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		_meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		msg.attachments
			.iter()
			.filter(|attachment| is_image(attachment))
			.fold(PatchExt::keep(), |patch, attachment| {
				patch.attachment(self.tooltip(attachment))
			})
	}
}

export_plugin!(ImageFilename, ext);
