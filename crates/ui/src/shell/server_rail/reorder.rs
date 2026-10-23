use gpui::{
	Context, FontWeight, IntoElement, ObjectFit, ParentElement as _, Render, SharedString, Styled as _,
	StyledImage as _, Window, div, img, prelude::FluentBuilder as _,
};
use media::SizeHint;
use protocol::Snowflake;
use state::RailEntry;

use crate::images::Images;
use crate::theme::Theme;

use super::{ICON_PIXELS, Id};

#[derive(Clone)]
pub(super) struct DraggedGuild {
	pub id: Snowflake,
	pub icon: Option<String>,
	pub acronym: SharedString,
}

pub(super) struct DragPreview {
	icon: Option<String>,
	acronym: SharedString,
}

impl DragPreview {
	pub fn new(dragged: &DraggedGuild) -> Self {
		Self {
			icon: dragged.icon.clone(),
			acronym: dragged.acronym.clone(),
		}
	}
}

impl Render for DragPreview {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let image = self
			.icon
			.as_deref()
			.and_then(|url| cx.global::<Images>().get(url, SizeHint::square(ICON_PIXELS)));
		div()
			.flex()
			.items_center()
			.justify_center()
			.size(theme.server_icon)
			.rounded(theme.radius_lg)
			.overflow_hidden()
			.opacity(0.85)
			.bg(theme.brand)
			.text_color(theme.text_on_brand)
			.text_size(theme.font_md)
			.font_weight(FontWeight::MEDIUM)
			.map(|preview| match image {
				Some(image) => preview.child(img(image).size_full().object_fit(ObjectFit::Cover)),
				None => preview.child(self.acronym.clone()),
			})
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Drop {
	BeforeGuild(Snowflake),
	BeforeFolder(Id),
	End,
	OntoGuild(Snowflake),
	OntoFolder(Id),
}

fn remove_guild(layout: &mut Vec<RailEntry>, moved: Snowflake) {
	layout.retain_mut(|entry| match entry {
		RailEntry::Guild(id) => *id != moved,
		RailEntry::Folder { guilds, .. } => {
			guilds.retain(|id| *id != moved);
			!guilds.is_empty()
		}
	});
}

fn top_level_guild(layout: &[RailEntry], guild: Snowflake) -> Option<usize> {
	layout.iter().position(|entry| *entry == RailEntry::Guild(guild))
}

fn folder_guilds(layout: &mut [RailEntry], target: Id) -> Option<&mut Vec<Snowflake>> {
	layout.iter_mut().find_map(|entry| match entry {
		RailEntry::Folder { id, guilds, .. } if *id == target => Some(guilds),
		_ => None,
	})
}

fn insert_guild(layout: &mut Vec<RailEntry>, moved: Snowflake, drop: Drop, folder_id: Id) -> Option<()> {
	match drop {
		Drop::End => layout.push(RailEntry::Guild(moved)),
		Drop::BeforeFolder(target) => {
			let at = layout
				.iter()
				.position(|entry| matches!(entry, RailEntry::Folder { id, .. } if *id == target))?;
			layout.insert(at, RailEntry::Guild(moved));
		}
		Drop::BeforeGuild(target) => match top_level_guild(layout, target) {
			Some(at) => layout.insert(at, RailEntry::Guild(moved)),
			None => {
				let guilds = layout.iter_mut().find_map(|entry| match entry {
					RailEntry::Folder { guilds, .. } if guilds.contains(&target) => Some(guilds),
					_ => None,
				})?;
				let at = guilds.iter().position(|id| *id == target)?;
				guilds.insert(at, moved);
			}
		},
		Drop::OntoGuild(target) => match top_level_guild(layout, target) {
			Some(at) => {
				layout[at] = RailEntry::Folder {
					id: folder_id,
					name: None,
					color: None,
					guilds: vec![target, moved],
				};
			}
			None => return insert_guild(layout, moved, Drop::BeforeGuild(target), folder_id),
		},
		Drop::OntoFolder(target) => folder_guilds(layout, target)?.push(moved),
	}
	Some(())
}

pub(super) fn rearranged(
	layout: &[RailEntry],
	moved: Snowflake,
	drop: Drop,
	folder_id: Id,
) -> Option<Vec<RailEntry>> {
	if drop == Drop::OntoGuild(moved) || drop == Drop::BeforeGuild(moved) {
		return None;
	}
	let mut next = layout.to_vec();
	remove_guild(&mut next, moved);
	insert_guild(&mut next, moved, drop, folder_id)?;
	(next != layout).then_some(next)
}

pub(super) fn new_folder_id() -> Id {
	chrono::Utc::now().timestamp_millis().unsigned_abs()
}
