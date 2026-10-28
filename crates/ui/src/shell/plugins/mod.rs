mod bridge;
mod chrome;
mod commands;
mod icon;
mod identity;
mod input;
mod menu;
mod message;
mod overlays;
mod settings;
mod view;

use std::rc::Rc;

use gpui::{AppContext as _, Context, Entity};

pub use self::bridge::{PluginBridge, PluginStore};
pub use self::chrome::{
	DebugPanel, RightPanel, SidebarPanel, app_nav, composer_toolbar, contextual, contextual_inline,
	guild_tooltip, main_view, mount_body, mount_column, mount_row, mount_titles, open_page, panel_toggle,
	right_panel_visible, status_bar, title_items,
};
pub use self::commands::{list as command_list, parse as parse_command};
pub use self::identity::{avatar_url, badges, channel_badges, decoration_of, display_name, tags};
pub use self::input::{
	LinkOrigin, autocomplete_declared, channel_click, client_options, defer_link, keybind,
	link_hook_declared, message_click, message_click_at, message_click_here, watching_messages,
};
pub use self::menu::{MenuTarget, PluginMenu, has_plugin_items, plugin_menu, plugin_menu_fill};
pub use self::message::{
	PatchEffects, Veil, author_badge, effects as patch_effects, extras as message_extras, message_actions,
};
pub use self::overlays::{PluginUi, PluginUiEvent};
pub use self::settings::{PluginPages, ThemeApplied};
pub use self::view::{INLINE, PANEL, resolve as resolve_color};
use super::SettingsBackend;
use crate::data::Data;

pub fn install<T: 'static>(
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	cx: &mut Context<T>,
) -> Entity<PluginUi> {
	let ui = cx.new({
		let (data, backend) = (data.clone(), backend.clone());
		move |cx| PluginUi::new(data, &backend, cx)
	});
	cx.set_global(PluginBridge::new(data, backend, ui.clone()));
	ui
}
