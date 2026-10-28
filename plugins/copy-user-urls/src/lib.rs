use plugin_sdk::model::UiContext;
use plugin_sdk::{Plugin, PluginExt, export_plugin, host, log, menu};

const COPY_ITEM: &str = "copy";

#[derive(Default)]
struct CopyUserUrls;

impl Plugin for CopyUserUrls {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = menu::set(menu::USER, [menu::item(COPY_ITEM, "Copy user URL").icon("link")]) {
			log::warn!("could not add the menu item: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for CopyUserUrls {
	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::USER || key != COPY_ITEM {
			return;
		}
		let Some(user_id) = context.user_id else {
			return;
		};
		if let Err(error) = host::clipboard_write(&format!("https://discord.com/users/{user_id}")) {
			log::warn!("could not copy the user URL: {error:?}");
		}
	}
}

export_plugin!(CopyUserUrls, ext);
