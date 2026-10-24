mod bridge;
mod chrome;
mod commands;
mod icon;
mod menu;
mod message;
mod overlays;
mod settings;
mod slot_renderer;

use std::rc::Rc;

use gpui::{AppContext as _, Context, Entity};

pub use self::bridge::{PluginBridge, PluginStore};
pub use self::chrome::{
	DebugPanel, RightPanel, SidebarPanel, composer_toolbar, panel_toggle, right_panel_visible, status_bar,
	title_items,
};
pub use self::commands::{list as command_list, parse as parse_command};
pub use self::menu::PluginMenu;
pub use self::message::{author_badge, extras as message_extras, message_actions};
pub use self::overlays::PluginUi;
pub use self::settings::{PluginPages, ThemeApplied};
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
