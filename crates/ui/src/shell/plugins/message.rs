use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
	AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
	StatefulInteractiveElement as _, Styled as _, div,
};
use plugin_host::SlotEntry;
use plugin_host::api::{UiBox, UiNode, UiTree};
use protocol::Snowflake;
use state::{MessageRow, MessageView};

use super::bridge::PluginBridge;
use super::slot_renderer::{MESSAGE_ACTION, Scope, SlotHost, render_node};
use crate::components::tooltip;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const ACCESSORY_SLOT: &str = "message-accessory";
const TARGET_PREFIXES: [&str; 2] = ["deleted-", "msg-"];
const CAPTION_ICON: f32 = 12.;

type Part = (usize, Option<u64>);

fn entries(slot: &str, cx: &App) -> Rc<Vec<SlotEntry>> {
	cx.global::<PluginBridge>().entries(slot, cx)
}

pub fn message_actions(message: Snowflake, cx: &App) -> Vec<AnyElement> {
	let host = SlotHost::new("message-action").compact(MESSAGE_ACTION);
	if host.entries(cx).is_empty() {
		return Vec::new();
	}
	host.payload(message.to_string())
		.render(cx)
		.into_iter()
		.map(|(_, element)| element)
		.collect()
}

fn box_of(node: &UiNode) -> Option<&UiBox> {
	match node {
		UiNode::Column(layout) | UiNode::Row(layout) => Some(layout),
		_ => None,
	}
}

fn target_of(layout: &UiBox) -> Option<u64> {
	let id = layout.id.as_deref()?;
	TARGET_PREFIXES
		.iter()
		.find_map(|prefix| id.strip_prefix(prefix))?
		.parse()
		.ok()
}

fn parts(tree: &UiTree) -> Vec<Part> {
	let mut parts = Vec::new();
	for root in &tree.roots {
		let index = *root as usize;
		let Some(layout) = tree.nodes.get(index).and_then(box_of) else {
			parts.push((index, None));
			continue;
		};
		if let Some(target) = target_of(layout) {
			parts.push((index, Some(target)));
			continue;
		}
		let children: Vec<Part> = layout
			.children
			.iter()
			.map(|child| {
				let child = *child as usize;
				(child, tree.nodes.get(child).and_then(box_of).and_then(target_of))
			})
			.collect();
		if children.iter().any(|(_, target)| target.is_some()) {
			parts.extend(children);
		} else {
			parts.push((index, None));
		}
	}
	parts
}

fn accessory_scope(entry: &SlotEntry, message: u64) -> Scope {
	Scope {
		slot: ACCESSORY_SLOT,
		plugin: entry.plugin_id.clone(),
		payload: message.to_string().into(),
		compact: None,
	}
}

fn accessories(message: Snowflake, theme: &Theme, cx: &App) -> Vec<AnyElement> {
	let entries = entries(ACCESSORY_SLOT, cx);
	let id = message.get();
	let mut out = Vec::new();
	for entry in entries.iter() {
		for (index, target) in parts(&entry.tree) {
			if target.is_none_or(|target| target == id) {
				let scope = accessory_scope(entry, id);
				out.push(
					div()
						.mt(theme.space_4)
						.child(render_node(&entry.tree, index, &scope, theme))
						.into_any_element(),
				);
			}
		}
	}
	out
}

pub fn orphan_accessories(rows: &[MessageRow], cx: &App) -> Option<AnyElement> {
	let entries = entries(ACCESSORY_SLOT, cx);
	if entries.is_empty() {
		return None;
	}
	let visible: HashSet<u64> = rows
		.iter()
		.filter_map(|row| match row {
			MessageRow::Message(view) => Some(view.message.id.get()),
			_ => None,
		})
		.collect();
	let theme = cx.global::<Theme>();
	let mut out = Vec::new();
	for entry in entries.iter() {
		for (index, target) in parts(&entry.tree) {
			let Some(target) = target.filter(|target| !visible.contains(target)) else {
				continue;
			};
			let scope = accessory_scope(entry, target);
			out.push(render_node(&entry.tree, index, &scope, theme));
		}
	}
	if out.is_empty() {
		return None;
	}
	Some(
		div()
			.flex()
			.flex_col()
			.flex_none()
			.gap(theme.space_8)
			.px(theme.space_16)
			.py(theme.space_8)
			.border_t_1()
			.border_color(theme.divider)
			.bg(theme.background_secondary)
			.children(out)
			.into_any_element(),
	)
}

fn caption(icon: IconKind, text: SharedString, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_4)
		.child(Icon::new(icon).size(rem(CAPTION_ICON)).color(theme.text_muted))
		.child(text)
		.into_any_element()
}

fn attribution(view: &MessageView, theme: &Theme, cx: &App) -> Option<AnyElement> {
	if view.plugin.is_none() && view.altered_by.is_none() {
		return None;
	}
	let bridge = cx.global::<PluginBridge>();
	let via = view.plugin.as_ref().map(|id| {
		let name = bridge.name_of(id, cx);
		caption(
			IconKind::Plugins,
			cx.t_args("plugin-via", &[("name", ArgValue::from(name.to_string()))]),
			theme,
		)
	});
	let altered = view.altered_by.as_ref().map(|id| {
		let name = bridge.name_of(id, cx);
		let hint = cx.t_args("plugin-edited-by", &[("name", ArgValue::from(name.to_string()))]);
		div()
			.id(("plugin-altered", view.message.id.get()))
			.flex()
			.items_center()
			.tooltip(tooltip(hint))
			.child(
				Icon::new(IconKind::Edit)
					.size(rem(CAPTION_ICON))
					.color(theme.text_muted),
			)
	});
	Some(
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.mt(rem(2.))
			.text_size(theme.font_tiny)
			.text_color(theme.text_muted)
			.children(via)
			.children(altered)
			.into_any_element(),
	)
}

pub fn extras(view: &MessageView, theme: &Theme, cx: &App) -> Vec<AnyElement> {
	let mut out = accessories(view.message.id, theme, cx);
	out.extend(attribution(view, theme, cx));
	if view.failed
		&& let Some(reason) = &view.failure_reason
	{
		out.push(
			div()
				.text_size(theme.font_sm)
				.text_color(theme.danger)
				.child(reason.clone())
				.into_any_element(),
		);
	}
	out
}

pub fn author_badge(view: &MessageView, theme: &Theme) -> Option<AnyElement> {
	let badge = view.author_badge.as_ref()?;
	Some(
		div()
			.flex_none()
			.px(theme.space_4)
			.rounded(theme.radius_sm)
			.bg(theme.mention_background)
			.text_color(theme.mention_text)
			.text_size(theme.font_tiny)
			.child(badge.clone())
			.into_any_element(),
	)
}
