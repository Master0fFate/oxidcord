use std::collections::{HashMap, HashSet};

use gpui::{
	AnyElement, ClickEvent, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use state::{DmSectionView, PluginBadge, UserOverride};

use super::plugins;
use crate::components::{Avatar, IconButton, MentionBadge, SectionHeader};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::model::{Status, status_of};
use crate::theme::{Theme, rem};

const AVATAR_PIXELS: u32 = 64;

pub enum DmSidebarEvent {
	OpenFriends,
	OpenDm(Snowflake),
	Search,
	OpenNitro,
	OpenShop,
	CreateGroup,
	CloseDm(Snowflake),
}

#[derive(PartialEq)]
struct DmItem {
	id: Snowflake,
	user: Option<Snowflake>,
	name: SharedString,
	avatar: String,
	status: Status,
	group_members: Option<usize>,
	unread: bool,
	mentions: u32,
	over: Option<UserOverride>,
	badges: Vec<PluginBadge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
	Friends,
	Nitro,
	Shop,
	Dm(Snowflake),
}

pub struct DmSidebar {
	data: Entity<Data>,
	dms: Vec<DmItem>,
	sections: Vec<DmSectionView>,
	folded: HashSet<(String, String)>,
	selected: Option<Selection>,
}

impl EventEmitter<DmSidebarEvent> for DmSidebar {}

impl DmSidebar {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let mut sidebar = Self {
			data,
			dms: Vec::new(),
			sections: Vec::new(),
			folded: HashSet::new(),
			selected: None,
		};
		sidebar.refresh(cx);
		sidebar
	}

	pub fn select(&mut self, dm: Option<Snowflake>, cx: &mut Context<Self>) {
		self.selected = Some(dm.map_or(Selection::Friends, Selection::Dm));
		cx.notify();
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		let dms: Vec<DmItem> = state
			.dm_rows()
			.into_iter()
			.map(|row| {
				let single = (row.recipients.len() == 1).then(|| row.recipients[0]);
				let over = single.and_then(|user| state.user_override(None, user).cloned());
				DmItem {
					id: row.channel_id,
					user: single,
					name: over
						.as_ref()
						.and_then(|over| over.name.clone())
						.unwrap_or(row.name)
						.into(),
					avatar: single.map_or_else(String::new, |user| {
						plugins::avatar_url(&state, None, user, AVATAR_PIXELS)
					}),
					status: single.map_or(Status::Offline, |user| status_of(&state, user)),
					group_members: single.is_none().then(|| row.recipients.len() + 1),
					unread: row.unread,
					mentions: row.mentions,
					over,
					badges: state
						.channel_decorations(row.channel_id)
						.iter()
						.flat_map(|decoration| decoration.badges.iter().cloned())
						.collect(),
				}
			})
			.collect();
		let sections = state.dm_sections().to_vec();
		drop(state);
		if dms != self.dms || sections != self.sections {
			self.dms = dms;
			self.sections = sections;
			cx.notify();
		}
	}

	fn toggle_section(&mut self, section: &DmSectionView, cx: &mut Context<Self>) {
		let key = (section.plugin_id.clone(), section.title.clone());
		if !self.folded.remove(&key) {
			self.folded.insert(key);
		}
		cx.notify();
	}

	fn nav_row(
		&self,
		id: &'static str,
		icon: IconKind,
		label: SharedString,
		target: Selection,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let selected = self.selected == Some(target);
		let (rest, hover) = if selected {
			(theme.interactive_active, theme.interactive_active)
		} else {
			(theme.text_muted, theme.interactive_hover)
		};
		let group: SharedString = format!("dm-nav-{id}").into();
		div()
			.id(id)
			.group(group.clone())
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.member_row_height)
			.mx(theme.space_8)
			.px(theme.space_8)
			.rounded(theme.radius_md)
			.cursor_pointer()
			.text_size(theme.font_md)
			.font_weight(FontWeight::MEDIUM)
			.text_color(rest)
			.when(selected, |row| row.bg(theme.modifier_selected))
			.when(!selected, |row| row.hover(|style| style.bg(theme.modifier_hover)))
			.on_click(cx.listener(move |this, _, _, cx| {
				this.selected = Some(target);
				match target {
					Selection::Friends => cx.emit(DmSidebarEvent::OpenFriends),
					Selection::Nitro => cx.emit(DmSidebarEvent::OpenNitro),
					Selection::Shop => cx.emit(DmSidebarEvent::OpenShop),
					Selection::Dm(_) => {}
				}
				cx.notify();
			}))
			.child(
				Icon::new(icon)
					.size(rem(24.))
					.color(rest)
					.group_hover(group, hover),
			)
			.child(label)
			.into_any_element()
	}

	fn dm_row(&self, dm: &DmItem, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = dm.id;
		let selected = self.selected == Some(Selection::Dm(id));
		let group: SharedString = format!("dm-{id}").into();
		let (name_color, close_color) = if selected {
			(theme.interactive_active, theme.interactive_active)
		} else if dm.unread {
			(theme.header_primary, theme.interactive_normal)
		} else {
			(theme.text_muted, theme.interactive_normal)
		};
		let avatar = match dm.group_members {
			None => Avatar::new(dm.name.clone())
				.size(rem(32.))
				.url(dm.avatar.clone())
				.status(dm.status)
				.decoration(plugins::decoration_of(dm.over.as_ref()))
				.cutout(if selected {
					theme.modifier_selected
				} else {
					theme.background_tertiary
				})
				.into_any_element(),
			Some(_) => div()
				.flex()
				.flex_none()
				.items_center()
				.justify_center()
				.size(rem(32.))
				.rounded_full()
				.bg(theme.brand)
				.child(
					Icon::new(IconKind::Group)
						.size(rem(18.))
						.color(theme.text_on_brand),
				)
				.into_any_element(),
		};
		let subtitle = dm
			.group_members
			.map(|members| cx.t_args("dm-group-members", &[("count", ArgValue::from(members))]));
		let tags: Vec<AnyElement> = match (&dm.over, dm.user) {
			(Some(over), Some(user)) => plugins::tags(over, user, theme, cx),
			_ => Vec::new(),
		};
		let channel_badges = plugins::channel_badges(id, &dm.badges, theme, cx);
		div()
			.id(("dm", id.get()))
			.group(group.clone())
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.member_row_height)
			.mx(theme.space_8)
			.px(theme.space_8)
			.rounded(theme.radius_md)
			.cursor_pointer()
			.when(selected, |row| row.bg(theme.modifier_selected))
			.when(!selected, |row| row.hover(|style| style.bg(theme.modifier_hover)))
			.on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
				if plugins::channel_click(None, id, event, cx) {
					return;
				}
				this.selected = Some(Selection::Dm(id));
				cx.emit(DmSidebarEvent::OpenDm(id));
				cx.notify();
			}))
			.on_aux_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
				if event.is_middle_click() {
					plugins::channel_click(None, id, event, cx);
				}
			}))
			.child(avatar)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.child(
						div()
							.flex()
							.items_center()
							.gap(theme.space_4)
							.min_w_0()
							.child(
								div()
									.truncate()
									.min_w_0()
									.text_size(theme.font_md)
									.line_height(rem(20.))
									.font_weight(if dm.unread || selected {
										FontWeight::SEMIBOLD
									} else {
										FontWeight::MEDIUM
									})
									.text_color(name_color)
									.child(dm.name.clone()),
							)
							.children(tags)
							.children(channel_badges),
					)
					.when_some(subtitle, |column, subtitle| {
						column.child(
							div()
								.truncate()
								.text_size(theme.font_tiny)
								.line_height(rem(14.))
								.text_color(theme.text_muted)
								.child(subtitle),
						)
					}),
			)
			.when(dm.mentions > 0, |row| row.child(MentionBadge::new(dm.mentions)))
			.child(
				div()
					.invisible()
					.group_hover(group.clone(), |style| style.visible())
					.child(
						IconButton::new(("dm-close", id.get()), IconKind::Close)
							.icon_size(rem(16.))
							.hit_size(rem(20.))
							.color(close_color)
							.tooltip(cx.t("dm-close"))
							.on_click(cx.listener(move |_, _, _, cx| {
								cx.stop_propagation();
								cx.emit(DmSidebarEvent::CloseDm(id));
							})),
					),
			)
			.into_any_element()
	}
}

impl DmSidebar {
	fn section_rows(&self, theme: &Theme, cx: &mut Context<Self>) -> (Vec<AnyElement>, HashSet<Snowflake>) {
		let mut placed: HashSet<Snowflake> = HashSet::new();
		let mut out: Vec<AnyElement> = Vec::new();
		if self.sections.is_empty() {
			return (out, placed);
		}
		let by_id: HashMap<Snowflake, &DmItem> = self.dms.iter().map(|dm| (dm.id, dm)).collect();
		for (index, section) in self.sections.iter().enumerate() {
			let items: Vec<&DmItem> = section
				.channels
				.iter()
				.filter(|id| placed.insert(**id))
				.filter_map(|id| by_id.get(id).copied())
				.collect();
			if items.is_empty() {
				continue;
			}
			let key = (section.plugin_id.clone(), section.title.clone());
			let collapsed = section.collapsed != self.folded.contains(&key);
			let target = section.clone();
			out.push(
				div()
					.pt(theme.space_16)
					.child(
						SectionHeader::new(
							SharedString::from(format!("dm-section-{index}")),
							section.title.clone(),
						)
						.collapsible(collapsed)
						.on_click(cx.listener(move |this, _, _, cx| this.toggle_section(&target, cx))),
					)
					.into_any_element(),
			);
			if !collapsed {
				out.extend(items.into_iter().map(|dm| self.dm_row(dm, theme, cx)));
			}
		}
		(out, placed)
	}
}

impl Render for DmSidebar {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (pinned, placed) = self.section_rows(&theme, cx);
		let rows: Vec<AnyElement> = self
			.dms
			.iter()
			.filter(|dm| !placed.contains(&dm.id))
			.map(|dm| self.dm_row(dm, &theme, cx))
			.collect();
		let top = plugins::mount_column("channel-list-top", cx);
		let friends = self.nav_row(
			"dm-friends",
			IconKind::Friends,
			cx.t("dm-friends"),
			Selection::Friends,
			&theme,
			cx,
		);
		div()
			.flex()
			.flex_col()
			.size_full()
			.min_h_0()
			.bg(theme.background_tertiary)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.h(theme.top_bar_height)
					.px(rem(10.))
					.border_b_1()
					.border_color(theme.divider)
					.child(
						div()
							.id("dm-search")
							.flex()
							.flex_1()
							.items_center()
							.h(rem(30.))
							.px(theme.space_8)
							.rounded(theme.radius_md)
							.bg(theme.background_secondary)
							.cursor_pointer()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.on_click(cx.listener(|_, _, _, cx| cx.emit(DmSidebarEvent::Search)))
							.child(div().truncate().child(cx.t("dm-search"))),
					),
			)
			.when(!top.is_empty(), |sidebar| {
				sidebar.child(
					div()
						.flex()
						.flex_col()
						.flex_none()
						.gap(theme.space_4)
						.px(theme.space_8)
						.pt(theme.space_8)
						.children(top),
				)
			})
			.child(
				div()
					.flex()
					.flex_col()
					.pt(theme.space_8)
					.pb(theme.space_16)
					.child(friends)
					.child(self.nav_row(
						"dm-nitro",
						IconKind::Nitro,
						cx.t("dm-nitro"),
						Selection::Nitro,
						&theme,
						cx,
					))
					.child(self.nav_row(
						"dm-shop",
						IconKind::Shop,
						cx.t("dm-shop"),
						Selection::Shop,
						&theme,
						cx,
					))
					.children(pinned)
					.child(
						div().pt(theme.space_16).child(
							SectionHeader::new("dm-header", cx.t("dm-direct-messages")).trailing(
								IconButton::new("dm-create", IconKind::Plus)
									.icon_size(rem(16.))
									.hit_size(rem(18.))
									.tooltip(cx.t("tooltip-create-dm"))
									.on_click(
										cx.listener(|_, _, _, cx| cx.emit(DmSidebarEvent::CreateGroup)),
									),
							),
						),
					)
					.children(rows)
					.overflow_y_scrollbar()
					.id("dm-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
