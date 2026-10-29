use plugin_sdk::model::{StickerInfo, UiContext};
use plugin_sdk::view;
use plugin_sdk::{HostError, Plugin, PluginExt, client, export_plugin, host, log, menu, mount, ui};

const COPY_ITEM: &str = "copy-link";
const OPEN_ITEM: &str = "open-link";
const PNG_FORMAT: u8 = 1;
const LINK_SIZE: u32 = 512;
const TOAST_MS: u32 = 3000;

#[derive(Default)]
struct CopyStickerLinks;

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

fn sticker_link(sticker_id: u64) -> String {
	let known = client::list_stickers(None)
		.unwrap_or_default()
		.into_iter()
		.find(|sticker| sticker.id == sticker_id);
	let sticker = known.unwrap_or(StickerInfo {
		id: sticker_id,
		name: String::new(),
		format: PNG_FORMAT,
		guild_id: None,
	});
	client::sticker_url(&sticker, LINK_SIZE)
}

impl Plugin for CopyStickerLinks {
	fn init(&mut self) -> Result<(), String> {
		let items = [
			menu::item(COPY_ITEM, "Copy link").icon("link"),
			menu::item(OPEN_ITEM, "Open link").icon("external-link"),
		];
		if let Err(error) = menu::set(menu::STICKER, items) {
			log::warn!("could not add the menu entries: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for CopyStickerLinks {
	fn on_menu_item(&mut self, menu: &str, key: &str, _context: &UiContext) {
		if menu != menu::STICKER {
			return;
		}
		let Some(sticker_id) = ui::context_detail().and_then(|detail| detail.sticker_id) else {
			return;
		};
		let link = sticker_link(sticker_id);
		let done = match key {
			COPY_ITEM => host::clipboard_write(&link).map(|()| "Link copied"),
			OPEN_ITEM => host::open_url(&link).map(|()| "Opening the link"),
			_ => return,
		};
		match done {
			Ok(text) => notice(text),
			Err(error) => notice(&describe(&error)),
		}
	}
}

export_plugin!(CopyStickerLinks, ext);
