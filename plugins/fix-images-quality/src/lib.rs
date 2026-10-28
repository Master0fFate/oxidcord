use plugin_sdk::model::{AttachmentInfo, AttachmentOverride, MessageFull, MessageMeta};
use plugin_sdk::patch::PatchExt;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, Plugin, PluginExt, export_plugin, storage};

const ORIGINAL_KEY: &str = "originalImagesInChat";
const PIXEL_LIMIT: u64 = 2000 * 1200;
const RESIZE_PARAMETERS: [&str; 4] = ["width", "height", "format", "quality"];

#[derive(Default)]
struct FixImagesQuality {
	original_in_chat: bool,
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

fn is_resize_parameter(pair: &str) -> bool {
	let name = pair.split('=').next().unwrap_or(pair);
	RESIZE_PARAMETERS.contains(&name)
}

fn original_url(url: &str) -> Option<String> {
	let (without_fragment, fragment) = match url.split_once('#') {
		Some((head, tail)) => (head, Some(tail)),
		None => (url, None),
	};
	let (path, query) = without_fragment.split_once('?').unwrap_or((without_fragment, ""));
	if !path.contains("/attachments/") {
		return None;
	}
	let kept: Vec<&str> = query
		.split('&')
		.filter(|pair| !pair.is_empty() && !is_resize_parameter(pair))
		.collect();
	let mut rebuilt = path.to_owned();
	if !kept.is_empty() {
		rebuilt.push('?');
		rebuilt.push_str(&kept.join("&"));
	}
	if let Some(fragment) = fragment {
		rebuilt.push('#');
		rebuilt.push_str(fragment);
	}
	Some(rebuilt)
}

fn within_budget(attachment: &AttachmentInfo) -> bool {
	match (attachment.width, attachment.height) {
		(Some(width), Some(height)) => u64::from(width) * u64::from(height) <= PIXEL_LIMIT,
		_ => true,
	}
}

impl FixImagesQuality {
	fn rewrite(&self, attachment: &AttachmentInfo) -> Option<AttachmentOverride> {
		let is_image = attachment
			.content_type
			.as_deref()
			.is_some_and(|kind| kind.starts_with("image/"));
		if !is_image || !(self.original_in_chat || within_budget(attachment)) {
			return None;
		}
		Some(AttachmentOverride {
			id: attachment.id,
			blur: false,
			alt: None,
			tooltip: None,
			url: Some(original_url(&attachment.url)?),
			hidden: false,
		})
	}
}

impl Plugin for FixImagesQuality {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{ORIGINAL_KEY}")) {
			self.original_in_chat = parse_flag(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == ORIGINAL_KEY
		{
			self.original_in_chat = parse_flag(&value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"originalImagesInChat":{"type":"boolean","title":"Original images in chat","description":"Also load images above 2000x1200 pixels from the original file. Animated images always animate and large ones may cause lag","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for FixImagesQuality {
	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		_meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		msg.attachments
			.iter()
			.filter_map(|attachment| self.rewrite(attachment))
			.fold(PatchExt::keep(), PatchExt::attachment)
	}
}

export_plugin!(FixImagesQuality, ext);
