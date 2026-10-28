use plugin_sdk::model::{Space, TextSize, UiContext};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{Event, HostError, Plugin, PluginExt, data, export_plugin, log, menu, mount, storage};

const FORMAT_KEY: &str = "format";
const SIZE_KEY: &str = "imgSize";
const AVATAR_ITEM: &str = "avatar";
const BANNER_ITEM: &str = "banner";
const ICON_ITEM: &str = "icon";
const SERVER_BANNER_ITEM: &str = "server-banner";
const CDN: &str = "https://cdn.discordapp.com";
const ORIGINAL_SIZE: &str = "4096";
const SQUARE_PX: u32 = 360;
const BANNER_WIDTH_PX: u32 = 480;
const BANNER_HEIGHT_PX: u32 = 192;
const TOAST_MS: u32 = 3000;

struct Config {
	format: String,
	size: String,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			format: "webp".to_owned(),
			size: "1024".to_owned(),
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		match key {
			FORMAT_KEY if matches!(value, "webp" | "png" | "jpg") => self.format = value.to_owned(),
			SIZE_KEY if matches!(value, "128" | "256" | "512" | "1024" | "2048" | "4096") => {
				self.size = value.to_owned();
			}
			_ => {}
		}
	}

	fn url(&self, kind: &str, owner: u64, hash: &str, size: &str) -> String {
		let extension = if hash.starts_with("a_") {
			"webp"
		} else {
			&self.format
		};
		format!("{CDN}/{kind}/{owner}/{hash}.{extension}?size={size}")
	}
}

#[derive(Default)]
struct ViewIcons {
	config: Config,
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

impl ViewIcons {
	fn show(&self, title: &str, kind: &str, owner: u64, hash: &str, banner: bool) {
		let (width, height) = if banner {
			(BANNER_WIDTH_PX, BANNER_HEIGHT_PX)
		} else {
			(SQUARE_PX, SQUARE_PX)
		};
		let url = self.config.url(kind, owner, hash, &self.config.size);
		let original = self.config.url(kind, owner, hash, ORIGINAL_SIZE);
		let caption = format!("{} px", self.config.size);
		let body = view::column()
			.gap(Space::Md)
			.child(view::image(url).image_size(width, height))
			.child(
				view::row()
					.gap(Space::Md)
					.child(view::text(caption).size(TextSize::Xs).muted())
					.child(view::link("Open", original)),
			);
		if let Err(error) = mount::open_modal(title, [body]) {
			log::warn!("could not open the image window: {error:?}");
		}
	}

	fn show_user(&self, context: &UiContext, banner: bool) {
		let Some(user_id) = context.user_id else {
			return;
		};
		let profile = match data::user_profile(user_id) {
			Ok(Some(profile)) => profile,
			Ok(None) => return notice("That profile is not available"),
			Err(error) => return notice(&describe(&error)),
		};
		if banner {
			match profile.banner {
				Some(hash) => self.show("Banner", "banners", user_id, &hash, true),
				None => notice("No banner"),
			}
		} else {
			match profile.user.avatar {
				Some(hash) => self.show("Avatar", "avatars", user_id, &hash, false),
				None => notice("No avatar"),
			}
		}
	}

	fn show_server(&self, context: &UiContext, banner: bool) {
		let Some(guild_id) = context.guild_id else {
			return;
		};
		let detail = match data::guild_detail(guild_id) {
			Ok(Some(detail)) => detail,
			Ok(None) => return notice("That server is not available"),
			Err(error) => return notice(&describe(&error)),
		};
		if banner {
			match detail.banner {
				Some(hash) => self.show("Server banner", "banners", guild_id, &hash, true),
				None => notice("No banner"),
			}
		} else {
			match detail.icon {
				Some(hash) => self.show("Server icon", "icons", guild_id, &hash, false),
				None => notice("No icon"),
			}
		}
	}
}

impl Plugin for ViewIcons {
	fn init(&mut self) -> Result<(), String> {
		for key in [FORMAT_KEY, SIZE_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		let menus = menu::set(
			menu::USER,
			[
				menu::item(AVATAR_ITEM, "View avatar").icon("image"),
				menu::item(BANNER_ITEM, "View banner").icon("image"),
			],
		)
		.and_then(|()| {
			menu::set(
				menu::SERVER,
				[
					menu::item(ICON_ITEM, "View icon").icon("image"),
					menu::item(SERVER_BANNER_ITEM, "View banner").icon("image"),
				],
			)
		});
		if let Err(error) = menus {
			log::warn!("could not add the menu entries: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"format":{"type":"string","title":"Image format","description":"The image format to use for non-animated images. Animated images always use webp","enum":["webp","png","jpg"],"default":"webp"},"imgSize":{"type":"string","title":"Image size","description":"The image size to load","enum":["128","256","512","1024","2048","4096"],"default":"1024"}}}"#
			.to_owned()
	}
}

impl PluginExt for ViewIcons {
	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		match (menu, key) {
			(menu::USER, AVATAR_ITEM) => self.show_user(context, false),
			(menu::USER, BANNER_ITEM) => self.show_user(context, true),
			(menu::SERVER, ICON_ITEM) => self.show_server(context, false),
			(menu::SERVER, SERVER_BANNER_ITEM) => self.show_server(context, true),
			_ => {}
		}
	}
}

export_plugin!(ViewIcons, ext);
