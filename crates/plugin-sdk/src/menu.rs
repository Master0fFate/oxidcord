use crate::bindings::oxidcord::plugin::ui;
use crate::model::{MenuItem, MenuItemExt, Tone};
use crate::types::HostError;

pub use crate::model::MenuWhen as When;
pub use crate::model::MenuWhenExt as WhenExt;

pub const MESSAGE: &str = "message-context-menu";
pub const USER: &str = "user-context-menu";
pub const CHANNEL: &str = "channel-context-menu";
pub const SERVER: &str = "server-context-menu";
pub const LINK: &str = "link-context-menu";
pub const EMOJI: &str = "emoji-context-menu";
pub const STICKER: &str = "sticker-context-menu";
pub const IMAGE: &str = "image-context-menu";
pub const ROLE: &str = "role-context-menu";
pub const SELECTION: &str = "selection-context-menu";
pub const COMPOSER: &str = "composer-context-menu";
pub const ACCOUNT_PANEL: &str = "account-panel-menu";
pub const ATTACHMENT: &str = "attachment-context-menu";
pub const REACTION: &str = "reaction-context-menu";

#[derive(Debug, Clone, PartialEq)]
pub struct Item(MenuItem);

#[derive(Debug, Clone, PartialEq)]
pub struct ItemExt(MenuItemExt);

pub fn item(key: impl Into<String>, label: impl Into<String>) -> Item {
	Item(MenuItem {
		key: key.into(),
		label: label.into(),
		icon: None,
		tone: Tone::Normal,
		when: When::empty(),
		parent: None,
	})
}

impl Item {
	pub fn icon(mut self, icon: impl Into<String>) -> Self {
		self.0.icon = Some(icon.into());
		self
	}

	pub fn tone(mut self, tone: Tone) -> Self {
		self.0.tone = tone;
		self
	}

	pub fn when(mut self, when: When) -> Self {
		self.0.when = when;
		self
	}

	pub fn parent(mut self, key: impl Into<String>) -> Self {
		self.0.parent = Some(key.into());
		self
	}
}

pub fn set(menu: &str, items: impl IntoIterator<Item = Item>) -> Result<(), HostError> {
	let items: Vec<MenuItem> = items.into_iter().map(|item| item.0).collect();
	ui::set_menu_items(menu, &items)
}

impl Item {
	pub fn extra(self, extra: WhenExt) -> ItemExt {
		ItemExt(MenuItemExt { base: self.0, extra })
	}
}

impl From<Item> for ItemExt {
	fn from(item: Item) -> Self {
		item.extra(WhenExt::empty())
	}
}

pub fn set_ext(menu: &str, items: impl IntoIterator<Item = ItemExt>) -> Result<(), HostError> {
	let items: Vec<MenuItemExt> = items.into_iter().map(|item| item.0).collect();
	ui::set_menu_items_ext(menu, &items)
}
