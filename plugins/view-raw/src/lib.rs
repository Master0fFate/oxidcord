mod json;
mod raw;

use plugin_sdk::model::{ButtonVariant, Length, Space, TextSize, UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, HostError, MessageRef, Plugin, PluginExt, Reply, client, data, export_plugin, host, log, menu,
	mount, storage,
};

const CLICK_METHOD_KEY: &str = "clickMethod";
const MESSAGE_MENU_KEY: &str = "messageContextMenu";
const SORT_KEYS_KEY: &str = "sortKeys";
const TOOLBAR_BUTTON: &str = "raw";
const MESSAGE_ITEM: &str = "raw-message";
const USER_ITEM: &str = "raw-user";
const CHANNEL_ITEM: &str = "raw-channel";
const MODAL_POINT: &str = "modal";
const COPY_DATA: &str = "copy-data";
const COPY_CONTENT: &str = "copy-content";
const CHUNK_BYTES: usize = 3500;
const MAX_CHUNKS: usize = 40;
const VIEW_HEIGHT: u32 = 380;
const TOAST_MS: u32 = 3000;

struct Config {
	menu_click: bool,
	message_menu: bool,
	sort_keys: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			menu_click: false,
			message_menu: false,
			sort_keys: true,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let on = !matches!(value.trim(), "false" | "0" | "off");
		match key {
			CLICK_METHOD_KEY => self.menu_click = value.trim() == "menu",
			MESSAGE_MENU_KEY => self.message_menu = matches!(value.trim(), "true" | "1" | "on"),
			SORT_KEYS_KEY => self.sort_keys = on,
			_ => {}
		}
	}
}

struct Open {
	json: String,
	content: Option<String>,
}

#[derive(Default)]
struct ViewRaw {
	config: Config,
	open: Option<Open>,
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

fn message_ref(context: &UiContext) -> Option<MessageRef> {
	Some(MessageRef {
		id: context.message_id?,
		channel_id: context.channel_id?,
		guild_id: context.guild_id,
	})
}

fn chunks(text: &str) -> Vec<&str> {
	let mut parts = Vec::new();
	let (mut start, mut end) = (0, 0);
	for line in text.split_inclusive('\n') {
		if end > start && end - start + line.len() > CHUNK_BYTES {
			parts.push(&text[start..end]);
			start = end;
		}
		end += line.len();
	}
	if end > start {
		parts.push(&text[start..end]);
	}
	parts
}

fn window(json: &str, has_content: bool) -> Vec<Node> {
	let parts = chunks(json);
	let mut body = view::column().gap(Space::Xs);
	for part in parts.iter().take(MAX_CHUNKS) {
		body = body.child(view::text(*part).mono().size(TextSize::Xs));
	}
	if parts.len() > MAX_CHUNKS {
		body = body.child(
			view::text("Shortened, copy it for the rest")
				.size(TextSize::Xs)
				.muted(),
		);
	}
	let mut buttons = view::row().gap(Space::Sm).child(
		view::button(COPY_DATA, "Copy data")
			.icon("copy")
			.variant(ButtonVariant::Outline),
	);
	if has_content {
		buttons = buttons.child(
			view::button(COPY_CONTENT, "Copy content")
				.icon("copy")
				.variant(ButtonVariant::Outline),
		);
	}
	vec![
		view::column()
			.gap(Space::Md)
			.fill()
			.child(buttons)
			.child(view::scroll().height(Length::Px(VIEW_HEIGHT)).fill().child(body)),
	]
}

impl ViewRaw {
	fn sync_surfaces(&self) {
		let toolbar = if self.config.menu_click {
			mount::unmount(mount::MESSAGE_ACTION, TOOLBAR_BUTTON)
		} else {
			mount::mount(
				mount::MESSAGE_ACTION,
				TOOLBAR_BUTTON,
				[view::icon_button(TOOLBAR_BUTTON, "braces", "View raw")],
				mount::Options::new("View raw"),
			)
		};
		if let Err(error) = toolbar {
			log::debug!("could not update the toolbar button: {error:?}");
		}
		let message_items = if self.config.menu_click || self.config.message_menu {
			vec![menu::item(MESSAGE_ITEM, "View raw")]
		} else {
			Vec::new()
		};
		let menus = menu::set(menu::MESSAGE, message_items)
			.and_then(|()| {
				menu::set(
					menu::USER,
					[menu::item(USER_ITEM, "View raw").when(menu::When::IN_GUILD)],
				)
			})
			.and_then(|()| {
				menu::set(
					menu::CHANNEL,
					[menu::item(CHANNEL_ITEM, "View raw").when(menu::When::IN_GUILD)],
				)
			});
		if let Err(error) = menus {
			log::warn!("could not update the menu entries: {error:?}");
		}
	}

	fn show(&mut self, title: &str, json: &json::Json, content: Option<String>) {
		let json = json.render(self.config.sort_keys);
		if let Err(error) = mount::open_modal(title, window(&json, content.is_some())) {
			log::warn!("could not open the window: {error:?}");
			return;
		}
		self.open = Some(Open { json, content });
	}

	fn show_message(&mut self, reference: MessageRef) {
		let message = match client::get_message(reference.channel_id, reference.id) {
			Ok(Some(message)) => message,
			Ok(None) => return notice("That message is not available"),
			Err(error) => return notice(&describe(&error)),
		};
		let meta = data::message_meta(reference).ok().flatten();
		self.show(
			"Raw message",
			&raw::message(&message, meta.as_ref()),
			Some(message.view.content),
		);
	}

	fn show_member(&mut self, context: &UiContext) {
		let (Some(guild_id), Some(user_id)) = (context.guild_id, context.user_id) else {
			return notice("Not in a server");
		};
		match data::member_info(guild_id, user_id) {
			Ok(Some(member)) => self.show("Raw member", &raw::member(&member), None),
			Ok(None) => notice("That member is not loaded"),
			Err(error) => notice(&describe(&error)),
		}
	}

	fn show_channel(&mut self, context: &UiContext) {
		let Some(channel_id) = context.channel_id else {
			return;
		};
		match data::channel_detail(channel_id) {
			Ok(Some(channel)) => self.show("Raw channel", &raw::channel(&channel), None),
			Ok(None) => notice("That channel is not available"),
			Err(error) => notice(&describe(&error)),
		}
	}

	fn copy(&self, node: &str) -> Reply {
		let Some(open) = &self.open else {
			return Reply::none();
		};
		let text = match node {
			COPY_DATA => Some(&open.json),
			COPY_CONTENT => open.content.as_ref(),
			_ => None,
		};
		match text.map(|text| host::clipboard_write(text)) {
			Some(Ok(())) => Reply::none().toast("Copied"),
			Some(Err(error)) => Reply::none().toast(describe(&error)),
			None => Reply::none(),
		}
	}
}

impl Plugin for ViewRaw {
	fn init(&mut self) -> Result<(), String> {
		for key in [CLICK_METHOD_KEY, MESSAGE_MENU_KEY, SORT_KEYS_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.sync_surfaces();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
			self.sync_surfaces();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"clickMethod":{"type":"string","title":"Message entry","description":"Where the message entry lives: button in the message toolbar, or menu in the message context menu","enum":["button","menu"],"default":"button"},"messageContextMenu":{"type":"boolean","title":"Message menu entry","description":"Also show the entry in the message context menu when the entry is the toolbar button","default":false},"sortKeys":{"type":"boolean","title":"Sort keys","description":"Alphabetically sort the keys of the raw data","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for ViewRaw {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click {
			return Reply::none();
		}
		match (event.point.as_str(), event.node.as_str()) {
			(mount::MESSAGE_ACTION, TOOLBAR_BUTTON) => {
				if let Some(reference) = message_ref(&event.context) {
					self.show_message(reference);
				}
				Reply::none()
			}
			(MODAL_POINT, node) => self.copy(node),
			_ => Reply::none(),
		}
	}

	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		match (menu, key) {
			(menu::MESSAGE, MESSAGE_ITEM) => {
				if let Some(reference) = message_ref(context) {
					self.show_message(reference);
				}
			}
			(menu::USER, USER_ITEM) => self.show_member(context),
			(menu::CHANNEL, CHANNEL_ITEM) => self.show_channel(context),
			_ => {}
		}
	}
}

export_plugin!(ViewRaw, ext);
