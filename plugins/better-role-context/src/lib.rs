use plugin_sdk::model::{Space, TextSize, UiContext};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{
	Event, HostError, Plugin, PluginExt, data, export_plugin, host, log, menu, mount, storage, ui,
};

const FORMAT_KEY: &str = "roleIconFileFormat";
const COLOR_ITEM: &str = "copy-color";
const ID_ITEM: &str = "copy-id";
const ICON_ITEM: &str = "view-icon";
const CDN: &str = "https://cdn.discordapp.com";
const ICON_PX: u32 = 128;
const ORIGINAL_SIZE: u32 = 1024;
const TOAST_MS: u32 = 3000;

struct BetterRoleContext {
	format: String,
}

impl Default for BetterRoleContext {
	fn default() -> Self {
		Self {
			format: "png".to_owned(),
		}
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

fn copy(text: &str) {
	match host::clipboard_write(text) {
		Ok(()) => notice("Copied"),
		Err(error) => notice(&describe(&error)),
	}
}

impl BetterRoleContext {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		if key == FORMAT_KEY && matches!(value, "png" | "webp" | "jpg") {
			self.format = value.to_owned();
		}
	}

	fn icon_url(&self, role_id: u64, hash: &str, size: u32) -> String {
		format!("{CDN}/role-icons/{role_id}/{hash}.{}?size={size}", self.format)
	}

	fn show_icon(&self, role_id: u64, hash: &str) {
		let body = view::column()
			.gap(Space::Md)
			.child(view::image(self.icon_url(role_id, hash, ICON_PX)).image_size(ICON_PX, ICON_PX))
			.child(
				view::row()
					.gap(Space::Md)
					.child(view::text(format!("{ICON_PX} px")).size(TextSize::Xs).muted())
					.child(view::link("Open", self.icon_url(role_id, hash, ORIGINAL_SIZE))),
			);
		if let Err(error) = mount::open_modal("Role icon", [body]) {
			log::warn!("could not open the icon window: {error:?}");
		}
	}

	fn with_role(&self, key: &str, guild_id: u64, role_id: u64) {
		let roles = match data::list_roles(guild_id) {
			Ok(roles) => roles,
			Err(error) => return notice(&describe(&error)),
		};
		let Some(role) = roles.into_iter().find(|role| role.id == role_id) else {
			return notice("That role is not available");
		};
		match (key, role.icon, role.color) {
			(COLOR_ITEM, _, 0) => notice("This role has no color"),
			(COLOR_ITEM, _, color) => copy(&format!("#{color:06x}")),
			(ICON_ITEM, Some(hash), _) => self.show_icon(role_id, &hash),
			(ICON_ITEM, None, _) => notice("This role has no icon"),
			_ => {}
		}
	}
}

impl Plugin for BetterRoleContext {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{FORMAT_KEY}")) {
			self.set(FORMAT_KEY, &String::from_utf8_lossy(&saved));
		}
		let items = [
			menu::item(COLOR_ITEM, "Copy color").icon("palette"),
			menu::item(ID_ITEM, "Copy role id").icon("copy"),
			menu::item(ICON_ITEM, "View icon").icon("image"),
		];
		if let Err(error) = menu::set(menu::ROLE, items) {
			log::warn!("could not add the menu entries: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"roleIconFileFormat":{"type":"string","title":"Icon format","description":"File format to use when viewing role icons","enum":["png","webp","jpg"],"default":"png"}}}"#
			.to_owned()
	}
}

impl PluginExt for BetterRoleContext {
	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::ROLE {
			return;
		}
		let Some(role_id) = ui::context_detail().and_then(|detail| detail.role_id) else {
			return;
		};
		if key == ID_ITEM {
			return copy(&role_id.to_string());
		}
		if let Some(guild_id) = context.guild_id {
			self.with_role(key, guild_id, role_id);
		}
	}
}

export_plugin!(BetterRoleContext, ext);
