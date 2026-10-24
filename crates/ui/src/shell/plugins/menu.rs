use std::sync::Arc;

use gpui::App;
use plugin_host::ViewContext;
use plugin_host::api::model::{MenuItem, MenuWhen, NodeKind, Tone};
use protocol::{Message, Snowflake};
use session::Command;

use super::bridge::{PluginBridge, act};
use crate::components::ContextMenuBuilder;
use crate::icons::IconKind;

#[derive(Clone, Default)]
pub struct MenuTarget {
	pub payload: String,
	pub context: ViewContext,
	pub url: Option<String>,
	pub when: MenuWhen,
}

fn place(guild: Option<Snowflake>) -> MenuWhen {
	if guild.is_some() {
		MenuWhen::IN_GUILD
	} else {
		MenuWhen::IN_DM
	}
}

impl MenuTarget {
	pub fn new(payload: impl ToString, context: ViewContext, when: MenuWhen) -> Self {
		Self {
			payload: payload.to_string(),
			context,
			url: None,
			when,
		}
	}

	pub fn server(guild: Snowflake) -> Self {
		let context = ViewContext {
			guild_id: Some(guild.get()),
			..ViewContext::default()
		};
		Self::new(guild, context, MenuWhen::IN_GUILD)
	}

	pub fn channel(guild: Option<Snowflake>, channel: Snowflake) -> Self {
		let context = ViewContext {
			guild_id: guild.map(Snowflake::get),
			channel_id: Some(channel.get()),
			..ViewContext::default()
		};
		Self::new(channel, context, place(guild))
	}

	pub fn user(guild: Option<Snowflake>, user: Snowflake, me: Option<Snowflake>) -> Self {
		let context = ViewContext {
			guild_id: guild.map(Snowflake::get),
			user_id: Some(user.get()),
			..ViewContext::default()
		};
		let who = if me == Some(user) {
			MenuWhen::SELF_USER
		} else {
			MenuWhen::OTHER_USER
		};
		Self::new(user, context, place(guild) | who)
	}

	pub fn message(message: &Message, guild: Option<Snowflake>, me: Option<Snowflake>) -> Self {
		let guild = message.guild_id.or(guild);
		let context = ViewContext {
			guild_id: guild.map(Snowflake::get),
			channel_id: Some(message.channel_id.get()),
			message_id: Some(message.id.get()),
			user_id: Some(message.author.id.get()),
		};
		let mut when = place(guild);
		when |= if me == Some(message.author.id) {
			MenuWhen::OWN_MESSAGE
		} else {
			MenuWhen::OTHERS_MESSAGE
		};
		if !message.content.is_empty() {
			when |= MenuWhen::HAS_CONTENT;
		}
		if !message.attachments.is_empty() {
			when |= MenuWhen::HAS_ATTACHMENTS;
		}
		Self::new(message.id, context, when)
	}
}

pub trait PluginMenu: Sized {
	fn plugin_items(self, menu: &'static str, target: MenuTarget, cx: &App) -> Self;
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

fn entry(
	menu: ContextMenuBuilder,
	item: &MenuItem,
	plugin: &Arc<str>,
	name: &'static str,
	target: &MenuTarget,
) -> ContextMenuBuilder {
	let icon = item.icon.as_deref().and_then(menu_icon);
	let plugin_id = plugin.to_string();
	let key = item.key.clone();
	let context = target.context.to_api(target.url.clone());
	let select = move |_: &mut gpui::Window, cx: &mut App| {
		cx.global::<PluginBridge>().send(
			Command::PluginMenuItem {
				plugin_id: plugin_id.clone(),
				menu: name.to_owned(),
				key: key.clone(),
				context: context.clone(),
			},
			cx,
		);
	};
	if item.tone == Tone::Danger {
		menu.danger_item(item.label.clone(), icon, select)
	} else {
		menu.item(item.label.clone(), icon, select)
	}
}

impl PluginMenu for ContextMenuBuilder {
	fn plugin_items(self, menu: &'static str, target: MenuTarget, cx: &App) -> Self {
		let bridge = cx.global::<PluginBridge>();
		let slots = bridge.slot_entries(menu, cx);
		let groups = bridge.menu_groups(menu, cx);
		let mut builder = self;
		let mut first = true;
		for surface in slots.iter() {
			for node in &surface.tree.nodes {
				let (NodeKind::Button(button), Some(key)) = (&node.kind, &node.key) else {
					continue;
				};
				if std::mem::take(&mut first) {
					builder = builder.separator();
				}
				let plugin = surface.plugin_id.clone();
				let key = key.clone();
				let payload = target.payload.clone();
				let icon = button.icon.as_deref().and_then(menu_icon);
				builder = builder.item(button.label.clone(), icon, move |_, cx| {
					act(&plugin, &key, &payload, cx)
				});
			}
		}
		for group in groups.iter() {
			let visible: Vec<&MenuItem> = group
				.items
				.iter()
				.filter(|item| target.when.contains(item.when))
				.collect();
			for item in visible.iter().filter(|item| item.parent.is_none()) {
				if std::mem::take(&mut first) {
					builder = builder.separator();
				}
				let children: Vec<&&MenuItem> = visible
					.iter()
					.filter(|child| child.parent.as_deref() == Some(item.key.as_str()))
					.collect();
				if children.is_empty() {
					builder = entry(builder, item, &group.plugin_id, menu, &target);
				} else {
					let submenu = children.iter().fold(ContextMenuBuilder::new(), |submenu, child| {
						entry(submenu, child, &group.plugin_id, menu, &target)
					});
					builder = builder.submenu(
						item.label.clone(),
						item.icon.as_deref().and_then(menu_icon),
						submenu,
					);
				}
			}
		}
		builder
	}
}
