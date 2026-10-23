use gpui::App;
use plugin_host::api::UiNode;

use super::bridge::{PluginBridge, act};
use crate::components::ContextMenuBuilder;
use crate::icons::IconKind;

pub trait PluginMenu: Sized {
	fn plugin_items(self, slot: &'static str, payload: String, cx: &App) -> Self;
}

fn menu_icon(name: &str) -> Option<IconKind> {
	Some(match name {
		"copy" => IconKind::Copy,
		"pencil" | "edit" => IconKind::Edit,
		"trash" => IconKind::Trash,
		"link" => IconKind::Link,
		"pin" => IconKind::Pin,
		"bell" => IconKind::Bell,
		"settings" => IconKind::Settings,
		"plus" => IconKind::Plus,
		"check" => IconKind::Check,
		"download" => IconKind::Download,
		"reply" => IconKind::Reply,
		"message-square" => IconKind::Chat,
		"users" => IconKind::Members,
		"user" => IconKind::Account,
		"search" => IconKind::Search,
		"image" => IconKind::Image,
		"folder" => IconKind::Folder,
		"external-link" => IconKind::ExternalLink,
		_ => return None,
	})
}

impl PluginMenu for ContextMenuBuilder {
	fn plugin_items(self, slot: &'static str, payload: String, cx: &App) -> Self {
		let entries = cx.global::<PluginBridge>().entries(slot, cx);
		let mut menu = self;
		let mut first = true;
		for entry in entries.iter() {
			for node in &entry.tree.nodes {
				let UiNode::Button(button) = node else {
					continue;
				};
				if std::mem::take(&mut first) {
					menu = menu.separator();
				}
				let plugin = entry.plugin_id.clone();
				let id = button.id.clone();
				let payload = payload.clone();
				let icon = button.icon.as_deref().and_then(menu_icon);
				menu = menu.item(button.label.clone(), icon, move |_, cx| {
					act(&plugin, &id, &payload, cx)
				});
			}
		}
		menu
	}
}
