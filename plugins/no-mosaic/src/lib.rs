use plugin_sdk::model::{AttachmentLayout, MessageFull, MessageMeta};
use plugin_sdk::patch::PatchExt;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, Plugin, PluginExt, export_plugin, storage};

const INLINE_VIDEO_KEY: &str = "inlineVideo";

struct NoMosaic {
	inline_video: bool,
}

impl Default for NoMosaic {
	fn default() -> Self {
		Self { inline_video: true }
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

fn has_video(msg: &MessageFull) -> bool {
	msg.attachments.iter().any(|attachment| {
		attachment
			.content_type
			.as_deref()
			.is_some_and(|kind| kind.starts_with("video/"))
	})
}

impl Plugin for NoMosaic {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{INLINE_VIDEO_KEY}")) {
			self.inline_video = parse_flag(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == INLINE_VIDEO_KEY
		{
			self.inline_video = parse_flag(&value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"inlineVideo":{"type":"boolean","title":"Inline video","description":"Play videos without carousel modal","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for NoMosaic {
	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		_meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		if msg.attachments.is_empty() {
			return PatchExt::keep();
		}
		let layout = if self.inline_video && has_video(msg) {
			AttachmentLayout::InlineVideo
		} else {
			AttachmentLayout::Stacked
		};
		PatchExt::keep().layout(layout)
	}
}

export_plugin!(NoMosaic, ext);
