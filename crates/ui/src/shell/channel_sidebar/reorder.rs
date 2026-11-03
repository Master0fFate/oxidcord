use gpui::{
	Context, FontWeight, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div,
};
use protocol::{ChannelPosition, Snowflake};

use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

#[derive(Clone)]
pub(super) struct DraggedChannel {
	pub id: Snowflake,
	pub category: bool,
	pub name: SharedString,
	pub icon: IconKind,
}

pub(super) struct DragPreview {
	name: SharedString,
	icon: IconKind,
}

impl DragPreview {
	pub fn new(dragged: &DraggedChannel) -> Self {
		Self {
			name: dragged.name.clone(),
			icon: dragged.icon,
		}
	}
}

impl Render for DragPreview {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.h(theme.channel_row_height)
			.px(theme.space_12)
			.rounded(theme.radius_md)
			.opacity(0.85)
			.bg(theme.background_secondary)
			.text_size(theme.font_md)
			.font_weight(FontWeight::MEDIUM)
			.text_color(theme.header_primary)
			.child(
				Icon::new(self.icon)
					.size(theme.glyph_sm)
					.color(theme.interactive_normal),
			)
			.child(self.name.clone())
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Drop {
	BeforeChannel(Snowflake),
	StartOf(Snowflake),
	EndOf(Option<Snowflake>),
	BeforeCategory(Snowflake),
}

pub(super) struct Slot {
	pub id: Snowflake,
	pub position: i64,
}

pub(super) struct Group {
	pub id: Option<Snowflake>,
	pub position: i64,
	pub channels: Vec<Slot>,
}

pub(super) fn rearranged(groups: &[Group], dragged: &DraggedChannel, drop: Drop) -> Vec<ChannelPosition> {
	if dragged.category {
		moved_category(groups, dragged.id, drop)
	} else {
		moved_channel(groups, dragged.id, drop)
	}
}

fn moved_category(groups: &[Group], moved: Snowflake, drop: Drop) -> Vec<ChannelPosition> {
	let mut order: Vec<(Snowflake, i64)> = groups
		.iter()
		.filter_map(|group| Some((group.id?, group.position)))
		.collect();
	let Some(from) = order.iter().position(|(id, _)| *id == moved) else {
		return Vec::new();
	};
	let entry = order.remove(from);
	let (anchor, after) = match drop {
		Drop::BeforeCategory(target) => (Some(target), false),
		Drop::StartOf(target) => (Some(target), true),
		Drop::EndOf(group) => (group, true),
		Drop::BeforeChannel(channel) => (
			groups
				.iter()
				.find(|group| group.channels.iter().any(|slot| slot.id == channel))
				.and_then(|group| group.id),
			true,
		),
	};
	let Some(at) = anchor.and_then(|target| order.iter().position(|(id, _)| *id == target)) else {
		return Vec::new();
	};
	order.insert(at + usize::from(after), entry);
	order
		.into_iter()
		.zip(0..)
		.filter(|((_, position), index)| position != index)
		.map(|((id, _), index)| ChannelPosition {
			id,
			position: index,
			parent_id: None,
		})
		.collect()
}

fn moved_channel(groups: &[Group], moved: Snowflake, drop: Drop) -> Vec<ChannelPosition> {
	let Some(source) = groups
		.iter()
		.find(|group| group.channels.iter().any(|slot| slot.id == moved))
	else {
		return Vec::new();
	};
	let target = match drop {
		Drop::BeforeChannel(channel) => groups
			.iter()
			.find(|group| group.channels.iter().any(|slot| slot.id == channel)),
		Drop::StartOf(category) => groups.iter().find(|group| group.id == Some(category)),
		Drop::EndOf(category) => groups.iter().find(|group| group.id == category),
		Drop::BeforeCategory(_) => None,
	};
	let Some(target) = target else {
		return Vec::new();
	};
	let mut order: Vec<&Slot> = target.channels.iter().filter(|slot| slot.id != moved).collect();
	let at = match drop {
		Drop::BeforeChannel(channel) => order.iter().position(|slot| slot.id == channel),
		Drop::StartOf(_) => Some(0),
		_ => Some(order.len()),
	};
	let Some(at) = at else {
		return Vec::new();
	};
	let Some(entry) = source.channels.iter().find(|slot| slot.id == moved) else {
		return Vec::new();
	};
	order.insert(at, entry);
	let regrouped = source.id != target.id;
	order
		.into_iter()
		.zip(0..)
		.filter(|(slot, index)| (slot.id == moved && regrouped) || slot.position != *index)
		.map(|(slot, index)| ChannelPosition {
			id: slot.id,
			position: index,
			parent_id: (slot.id == moved && regrouped).then_some(target.id),
		})
		.collect()
}
