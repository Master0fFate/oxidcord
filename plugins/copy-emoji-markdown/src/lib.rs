use plugin_sdk::model::{EmojiRef, UiContext};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{Event, HostError, Plugin, PluginExt, export_plugin, host, log, menu, mount, storage, ui};

const COPY_UNICODE_KEY: &str = "copyUnicode";
const MENU_ITEM: &str = "copy";
const TOAST_MS: u32 = 3000;

struct CopyEmojiMarkdown {
	copy_unicode: bool,
}

impl Default for CopyEmojiMarkdown {
	fn default() -> Self {
		Self { copy_unicode: true }
	}
}

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn notice(text: &str) {
	if let Err(error) = mount::toast([view::text(text)], TOAST_MS) {
		log::debug!("could not show the toast: {error:?}");
	}
}

fn markdown(emoji: &EmojiRef) -> String {
	let Some(id) = emoji.id else {
		return emoji.name.clone();
	};
	let name = match emoji.name.rsplit_once('~') {
		Some((base, tail)) if !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()) => base,
		_ => &emoji.name,
	};
	let animated = if emoji.animated { "a" } else { "" };
	format!("<{animated}:{name}:{id}>")
}

impl Plugin for CopyEmojiMarkdown {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{COPY_UNICODE_KEY}")) {
			self.copy_unicode = !matches!(String::from_utf8_lossy(&saved).trim(), "false" | "0" | "off");
		}
		if let Err(error) = menu::set(menu::EMOJI, [menu::item(MENU_ITEM, "Copy emoji").icon("copy")]) {
			log::warn!("could not add the menu entry: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == COPY_UNICODE_KEY
		{
			self.copy_unicode = !matches!(value.trim(), "false" | "0" | "off");
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"copyUnicode":{"type":"boolean","title":"Copy unicode","description":"Copy the raw unicode character instead of :name: for default emojis","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for CopyEmojiMarkdown {
	fn on_menu_item(&mut self, menu: &str, key: &str, _context: &UiContext) {
		if menu != menu::EMOJI || key != MENU_ITEM {
			return;
		}
		let Some(emoji) = ui::context_detail().and_then(|detail| detail.emoji) else {
			return;
		};
		if emoji.id.is_none() && !self.copy_unicode {
			return notice("Copying unicode emoji is off");
		}
		match host::clipboard_write(&markdown(&emoji)) {
			Ok(()) => notice("Copied"),
			Err(error) => notice(&describe(&error)),
		}
	}
}

export_plugin!(CopyEmojiMarkdown, ext);
