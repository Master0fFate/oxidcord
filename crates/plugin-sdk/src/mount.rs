use crate::bindings::oxidcord::plugin::ui;
use crate::model::MountOptions;
use crate::types::HostError;
use crate::view::{self, Node};

pub const TITLE_BAR: &str = "title-bar";
pub const STATUS_BAR: &str = "status-bar";
pub const COMPOSER_TOOLBAR: &str = "composer-toolbar";
pub const MESSAGE_ACTION: &str = "message-action";
pub const SIDEBAR_PANEL: &str = "sidebar-panel";
pub const RIGHT_PANEL: &str = "right-panel";
pub const APP_NAV: &str = "app-nav";
pub const MAIN_VIEW: &str = "main-view";
pub const SETTINGS_PAGE: &str = "settings-page";
pub const CHANNEL_HEADER: &str = "channel-header";
pub const COMPOSER_ABOVE: &str = "composer-above";
pub const USER_POPOUT_SECTION: &str = "user-popout-section";
pub const MEMBER_LIST_SECTION: &str = "member-list-section";
pub const ACCOUNT_PANEL: &str = "account-panel";
pub const SERVER_RAIL_TOP: &str = "server-rail-top";
pub const VOICE_PANEL: &str = "voice-panel";
pub const FRIENDS_SECTION: &str = "friends-section";
pub const CHANNEL_LIST_TOP: &str = "channel-list-top";
pub const ATTACHMENT_ACTION: &str = "attachment-action";
pub const REPLY_PREVIEW: &str = "reply-preview";
pub const GUILD_TOOLTIP: &str = "guild-tooltip";
pub const MEMBER_LIST_HEADER: &str = "member-list-header";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
	title: String,
	icon: Option<String>,
	order: i32,
}

impl Options {
	pub fn new(title: impl Into<String>) -> Self {
		Self {
			title: title.into(),
			icon: None,
			order: 0,
		}
	}

	pub fn icon(mut self, icon: impl Into<String>) -> Self {
		self.icon = Some(icon.into());
		self
	}

	pub fn order(mut self, order: i32) -> Self {
		self.order = order;
		self
	}

	fn into_api(self) -> MountOptions {
		MountOptions {
			title: self.title,
			icon: self.icon,
			order: self.order,
		}
	}
}

pub fn mount<N: Into<Node>>(
	point: &str,
	key: &str,
	roots: impl IntoIterator<Item = N>,
	options: Options,
) -> Result<(), HostError> {
	ui::mount(point, key, &view::tree(roots), &options.into_api())
}

pub fn update<N: Into<Node>>(
	point: &str,
	key: &str,
	roots: impl IntoIterator<Item = N>,
) -> Result<(), HostError> {
	ui::update(point, key, &view::tree(roots))
}

pub fn unmount(point: &str, key: &str) -> Result<(), HostError> {
	ui::unmount(point, key)
}

pub fn provide(point: &str, options: Options) -> Result<(), HostError> {
	ui::provide(point, &options.into_api())
}

pub fn invalidate(point: &str) -> Result<(), HostError> {
	ui::invalidate(point)
}

pub fn toast<N: Into<Node>>(roots: impl IntoIterator<Item = N>, ttl_ms: u32) -> Result<(), HostError> {
	ui::toast(&view::tree(roots), ttl_ms)
}

pub fn open_modal<N: Into<Node>>(title: &str, roots: impl IntoIterator<Item = N>) -> Result<(), HostError> {
	ui::open_modal(title, &view::tree(roots))
}
