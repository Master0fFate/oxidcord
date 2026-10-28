use std::sync::Arc;

use gpui::{
	AnyElement, App, ElementId, InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _,
	div,
};
use gpui_component::menu::ContextMenuExt as _;
use plugin_host::ViewContext;
use plugin_host::api::model::{ContextDetail, EmojiRef, MenuItem, MenuWhen, MenuWhenExt, NodeKind, Tone};
use protocol::{Message, Snowflake};
use session::Command;

use super::bridge::{MenuGroup, PluginBridge, act};
use crate::components::ContextMenuBuilder;
use crate::icons::IconKind;

#[derive(Clone, Default)]
pub struct MenuTarget {
	pub payload: String,
	pub context: ViewContext,
	pub url: Option<String>,
	pub when: MenuWhen,
	pub ext: MenuWhenExt,
	pub detail: Option<ContextDetail>,
}

pub fn blank_detail() -> ContextDetail {
	ContextDetail {
		emoji: None,
		sticker_id: None,
		role_id: None,
		attachment_id: None,
		selection: None,
		image_url: None,
		reaction: None,
	}
}

fn place(guild: Option<Snowflake>) -> MenuWhen {
	if guild.is_some() {
		MenuWhen::IN_GUILD
	} else {
		MenuWhen::IN_DM
	}
}

fn scope(guild: Option<Snowflake>, channel: Option<Snowflake>) -> ViewContext {
	ViewContext {
		guild_id: guild.map(Snowflake::get),
		channel_id: channel.map(Snowflake::get),
		..ViewContext::default()
	}
}

impl MenuTarget {
	pub fn new(payload: impl ToString, context: ViewContext, when: MenuWhen) -> Self {
		Self {
			payload: payload.to_string(),
			context,
			url: None,
			when,
			ext: MenuWhenExt::empty(),
			detail: None,
		}
	}

	pub fn with_detail(mut self, detail: ContextDetail) -> Self {
		self.detail = Some(detail);
		self
	}

	pub fn with_ext(mut self, ext: MenuWhenExt) -> Self {
		self.ext |= ext;
		self
	}

	pub fn with_url(mut self, url: impl Into<String>) -> Self {
		self.url = Some(url.into());
		self
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
			attachment_id: None,
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

	pub fn emoji(guild: Option<Snowflake>, channel: Option<Snowflake>, emoji: EmojiRef) -> Self {
		let payload = emoji.id.map_or_else(|| emoji.name.clone(), |id| id.to_string());
		Self::new(payload, scope(guild, channel), place(guild)).with_detail(ContextDetail {
			emoji: Some(emoji),
			..blank_detail()
		})
	}

	pub fn sticker(guild: Option<Snowflake>, channel: Option<Snowflake>, sticker: Snowflake) -> Self {
		Self::new(sticker, scope(guild, channel), place(guild)).with_detail(ContextDetail {
			sticker_id: Some(sticker.get()),
			..blank_detail()
		})
	}

	pub fn role(guild: Snowflake, role: Snowflake, own: bool) -> Self {
		let target =
			Self::new(role, scope(Some(guild), None), MenuWhen::IN_GUILD).with_detail(ContextDetail {
				role_id: Some(role.get()),
				..blank_detail()
			});
		if own {
			target.with_ext(MenuWhenExt::OWN_ROLE)
		} else {
			target
		}
	}

	pub fn selecting(mut self, text: String) -> Self {
		let mut detail = self.detail.take().unwrap_or_else(blank_detail);
		detail.selection = Some(text);
		self.detail = Some(detail);
		self.with_ext(MenuWhenExt::HAS_SELECTION)
	}

	pub fn composer(guild: Option<Snowflake>, channel: Option<Snowflake>, selection: String) -> Self {
		let target = Self::new("", scope(guild, channel), place(guild));
		if selection.is_empty() {
			target
		} else {
			target.selecting(selection)
		}
	}

	pub fn account() -> Self {
		Self::new("", ViewContext::default(), MenuWhen::SELF_USER)
	}

	pub fn image(mut self, url: &str) -> Self {
		let mut detail = self.detail.take().unwrap_or_else(blank_detail);
		detail.image_url = Some(url.to_owned());
		self.detail = Some(detail);
		self.with_ext(MenuWhenExt::IS_IMAGE)
	}

	pub fn attachment(mut self, attachment: Snowflake) -> Self {
		let mut detail = self.detail.take().unwrap_or_else(blank_detail);
		detail.attachment_id = Some(attachment.get());
		self.context.attachment_id = Some(attachment.get());
		self.detail = Some(detail);
		self
	}

	pub fn reaction(mut self, emoji: EmojiRef) -> Self {
		let mut detail = self.detail.take().unwrap_or_else(blank_detail);
		detail.reaction = Some(emoji);
		self.detail = Some(detail);
		self
	}
}

pub trait PluginMenu: Sized {
	fn plugin_items(self, menu: &'static str, target: MenuTarget, cx: &App) -> Self;
}

fn in_voice(cx: &App) -> bool {
	cx.global::<PluginBridge>()
		.data()
		.read(cx)
		.state()
		.voice()
		.is_some()
}

fn shown(group: &MenuGroup, item: &MenuItem, target: &MenuTarget, voice: bool) -> bool {
	if !target.when.contains(item.when) {
		return false;
	}
	let Some((_, wanted)) = group.extra.iter().find(|(key, _)| *key == item.key) else {
		return true;
	};
	let mut held = target.ext;
	if voice {
		held |= MenuWhenExt::IN_VOICE;
	}
	held.contains(*wanted)
}

pub fn has_plugin_items(menu: &'static str, target: &MenuTarget, cx: &App) -> bool {
	if !cx.has_global::<PluginBridge>() {
		return false;
	}
	let bridge = cx.global::<PluginBridge>();
	if !bridge.slot_entries(menu, cx).is_empty() {
		return true;
	}
	let groups = bridge.menu_groups(menu, cx);
	if groups.is_empty() {
		return false;
	}
	let voice = in_voice(cx);
	groups
		.iter()
		.any(|group| group.items.iter().any(|item| shown(group, item, target, voice)))
}

pub fn plugin_menu(
	id: impl Into<ElementId>,
	element: AnyElement,
	menu: &'static str,
	target: MenuTarget,
	cx: &App,
) -> AnyElement {
	wrapped(id, element, menu, target, false, cx)
}

pub fn plugin_menu_fill(
	id: impl Into<ElementId>,
	element: AnyElement,
	menu: &'static str,
	target: MenuTarget,
	cx: &App,
) -> AnyElement {
	wrapped(id, element, menu, target, true, cx)
}

fn wrapped(
	id: impl Into<ElementId>,
	element: AnyElement,
	menu: &'static str,
	target: MenuTarget,
	fill: bool,
	cx: &App,
) -> AnyElement {
	if !has_plugin_items(menu, &target, cx) {
		return element;
	}
	let items = ContextMenuBuilder::new().plugin_items(menu, target, cx);
	let holder = div().id(id);
	let holder = if fill {
		holder.flex().flex_1().min_w_0()
	} else {
		holder.flex_none()
	};
	holder
		.child(element)
		.context_menu(items.build())
		.into_any_element()
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
	let detail = target.detail.clone();
	let select = move |_: &mut gpui::Window, cx: &mut App| {
		cx.global::<PluginBridge>().send(
			Command::PluginMenuItem {
				plugin_id: plugin_id.clone(),
				menu: name.to_owned(),
				key: key.clone(),
				context: context.clone(),
				detail: detail.clone(),
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
		let voice = in_voice(cx);
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
				.filter(|item| shown(group, item, &target, voice))
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
