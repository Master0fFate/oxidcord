use crate::bindings::oxidcord::plugin::ui;
use crate::model::{MenuItem, Tone};
use crate::types::HostError;

pub use crate::model::MenuWhen as When;

pub const MESSAGE: &str = "message-context-menu";
pub const USER: &str = "user-context-menu";
pub const CHANNEL: &str = "channel-context-menu";
pub const SERVER: &str = "server-context-menu";
pub const LINK: &str = "link-context-menu";

#[derive(Debug, Clone, PartialEq)]
pub struct Item(MenuItem);

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
