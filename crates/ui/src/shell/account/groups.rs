use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, ParentElement as _, Pixels, Point, Render, SharedString, StatefulInteractiveElement as _,
	Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::scroll::ScrollableElement as _;
use protocol::{ChannelType, Snowflake};
use session::Command;

use super::picker;
use crate::components::{Avatar, ContextMenuBuilder, IconButton};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::lookup::avatar_for;
use crate::model::{Status, status_of};
use crate::theme::{Theme, rem};

const AVATAR_PIXELS: u32 = 64;

pub enum GroupEvent {
	OpenProfile { user: Snowflake, at: Point<Pixels> },
}

pub struct Groups {
	pub bar: Entity<GroupBar>,
	pub members: Entity<GroupMembers>,
}

impl Groups {
	pub fn new<T: 'static>(data: Entity<Data>, window: &mut Window, cx: &mut Context<T>) -> Self {
		Self {
			bar: cx.new(|cx| GroupBar::new(data.clone(), window, cx)),
			members: cx.new(|_| GroupMembers::new(data)),
		}
	}

	pub fn show<T: 'static>(&self, channel: Option<Snowflake>, cx: &mut Context<T>) {
		self.bar.update(cx, |bar, cx| bar.show(channel, cx));
		self.members.update(cx, |members, cx| members.show(channel, cx));
	}

	pub fn refresh<T: 'static>(&self, cx: &mut Context<T>) {
		self.bar.update(cx, |bar, cx| bar.refresh(cx));
		self.members.update(cx, |members, cx| members.refresh(cx));
	}

	pub fn active(&self, cx: &gpui::App) -> bool {
		self.bar.read(cx).group.is_some()
	}
}

#[derive(PartialEq)]
struct Group {
	channel: Snowflake,
	name: SharedString,
	owner: Option<Snowflake>,
	members: Vec<Snowflake>,
}

fn load_group(data: &Data, channel: Option<Snowflake>) -> Option<Group> {
	let channel = channel?;
	let state = data.state();
	let found = state.channel(channel)?;
	if found.kind != ChannelType::GroupDm {
		return None;
	}
	let name = state
		.dm_rows()
		.into_iter()
		.find(|row| row.channel_id == channel)
		.map(|row| row.name)
		.unwrap_or_default();
	Some(Group {
		channel,
		name: name.into(),
		owner: found.owner_id,
		members: found.recipient_ids.clone(),
	})
}

pub struct GroupBar {
	data: Entity<Data>,
	channel: Option<Snowflake>,
	group: Option<Group>,
	editing: bool,
	input: Entity<InputState>,
	_subscription: Subscription,
}

impl GroupBar {
	fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let input = cx.new(|cx| InputState::new(window, cx));
		let subscription =
			cx.subscribe_in(
				&input,
				window,
				|this, input, event: &InputEvent, _, cx| match event {
					InputEvent::PressEnter { .. } => {
						let name = input.read(cx).value().trim().to_owned();
						this.rename(name, cx);
					}
					InputEvent::Blur => {
						this.editing = false;
						cx.notify();
					}
					_ => {}
				},
			);
		Self {
			data,
			channel: None,
			group: None,
			editing: false,
			input,
			_subscription: subscription,
		}
	}

	fn show(&mut self, channel: Option<Snowflake>, cx: &mut Context<Self>) {
		self.channel = channel;
		self.editing = false;
		self.refresh(cx);
	}

	fn refresh(&mut self, cx: &mut Context<Self>) {
		let group = load_group(self.data.read(cx), self.channel);
		if group != self.group {
			self.group = group;
			cx.notify();
		}
	}

	fn rename(&mut self, name: String, cx: &mut Context<Self>) {
		self.editing = false;
		if let Some(group) = &self.group
			&& !name.is_empty()
			&& name != group.name.as_ref()
		{
			self.data.read(cx).send(Command::RenameGroup {
				channel_id: group.channel,
				name,
			});
		}
		cx.notify();
	}

	fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(group) = &self.group else {
			return;
		};
		let name = group.name.clone();
		self.editing = true;
		self.input.update(cx, |state, cx| {
			state.set_value(name, window, cx);
			state.focus(window, cx);
		});
		cx.notify();
	}
}

impl Render for GroupBar {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let Some(group) = &self.group else {
			return div().into_any_element();
		};
		let channel = group.channel;
		let count = group.members.len() + 1;
		let name = if self.editing {
			div()
				.w(theme.settings_card_width)
				.child(Input::new(&self.input))
				.into_any_element()
		} else {
			div()
				.id("group-name")
				.flex()
				.items_center()
				.gap(theme.space_8)
				.cursor_pointer()
				.on_click(cx.listener(|this, _, window, cx| this.begin_rename(window, cx)))
				.child(
					div()
						.truncate()
						.text_size(theme.font_md)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(theme.header_primary)
						.child(group.name.clone()),
				)
				.child(
					Icon::new(IconKind::Edit)
						.size(theme.icon_sm)
						.color(theme.interactive_normal),
				)
				.into_any_element()
		};
		let data = self.data.clone();
		let leave_data = self.data.clone();
		div()
			.flex()
			.flex_1()
			.min_w_0()
			.items_center()
			.gap(theme.space_8)
			.child(Icon::new(IconKind::Group).size(rem(22.)).color(theme.text_muted))
			.child(name)
			.child(
				div()
					.flex_1()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t_args("dm-group-members", &[("count", ArgValue::from(count))])),
			)
			.child(
				IconButton::new("group-add-member", IconKind::FriendRequests)
					.filled_hover()
					.tooltip(cx.t("group-add-title"))
					.on_click(move |_, window, cx| picker::open_add(data.clone(), channel, window, cx)),
			)
			.child(
				IconButton::new("group-leave", IconKind::LogOut)
					.filled_hover()
					.tooltip(cx.t("group-leave"))
					.color(theme.danger)
					.on_click(move |_, window, cx| {
						picker::confirm_close(leave_data.clone(), channel, true, window, cx);
					}),
			)
			.into_any_element()
	}
}

#[derive(PartialEq)]
struct MemberItem {
	id: Snowflake,
	name: SharedString,
	avatar: String,
	status: Status,
	owner: bool,
}

pub struct GroupMembers {
	data: Entity<Data>,
	channel: Option<Snowflake>,
	items: Vec<MemberItem>,
	can_remove: bool,
}

impl EventEmitter<GroupEvent> for GroupMembers {}

impl GroupMembers {
	fn new(data: Entity<Data>) -> Self {
		Self {
			data,
			channel: None,
			items: Vec::new(),
			can_remove: false,
		}
	}

	fn show(&mut self, channel: Option<Snowflake>, cx: &mut Context<Self>) {
		self.channel = channel;
		self.refresh(cx);
	}

	fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let Some(group) = load_group(data, self.channel) else {
			if !self.items.is_empty() || self.can_remove {
				self.items.clear();
				self.can_remove = false;
				cx.notify();
			}
			return;
		};
		let state = data.state();
		let me = state.current_user_id();
		let can_remove = me.is_some() && group.owner == me;
		let mut ids = group.members.clone();
		ids.extend(me);
		let items: Vec<MemberItem> = ids
			.into_iter()
			.map(|id| MemberItem {
				id,
				name: state.display_name(None, id).into(),
				avatar: avatar_for(&state, id, AVATAR_PIXELS),
				status: status_of(&state, id),
				owner: group.owner == Some(id),
			})
			.collect();
		drop(state);
		if items != self.items || can_remove != self.can_remove {
			self.items = items;
			self.can_remove = can_remove;
			cx.notify();
		}
	}

	fn row(
		&self,
		item: &MemberItem,
		me: Option<Snowflake>,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = item.id;
		let channel = self.channel;
		let data = self.data.clone();
		let remove_data = self.data.clone();
		let weak = cx.weak_entity();
		let mut menu = ContextMenuBuilder::new()
			.item(
				cx.t("menu-profile"),
				Some(IconKind::Profile),
				move |window, cx| {
					let at = window.mouse_position();
					weak.update(cx, |_, cx| cx.emit(GroupEvent::OpenProfile { user: id, at }))
						.ok();
				},
			)
			.item(cx.t("menu-message"), Some(IconKind::Chat), move |_, cx| {
				data.read(cx).send(Command::OpenDm { user_id: id });
			})
			.separator()
			.item(cx.t("menu-copy-user-id"), Some(IconKind::Copy), move |_, _| {
				if let Err(error) = platform::clipboard::write_text(&id.to_string()) {
					tracing::warn!(%error, "could not copy the user id");
				}
			});
		if self.can_remove
			&& Some(id) != me
			&& let Some(channel_id) = channel
		{
			menu = menu.separator().danger_item(
				cx.t("group-remove-member"),
				Some(IconKind::Trash),
				move |_, cx| {
					remove_data.read(cx).send(Command::RemoveGroupRecipient {
						channel_id,
						user_id: id,
					});
				},
			);
		}
		div()
			.id(("group-member", id.get()))
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.member_row_height)
			.mx(theme.space_8)
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_hover))
			.on_click(cx.listener(move |_, event: &gpui::ClickEvent, _, cx| {
				cx.emit(GroupEvent::OpenProfile {
					user: id,
					at: event.position(),
				})
			}))
			.context_menu(menu.build())
			.child(
				Avatar::new(item.name.clone())
					.size(theme.avatar_sm)
					.url(item.avatar.clone())
					.status(item.status)
					.cutout(theme.background_secondary),
			)
			.child(
				div()
					.flex_1()
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::MEDIUM)
					.text_color(theme.header_primary)
					.child(item.name.clone()),
			)
			.when(item.owner, |row| {
				row.child(
					div()
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.child(cx.t("group-owner")),
				)
			})
			.into_any_element()
	}
}

impl Render for GroupMembers {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let me = self.data.read(cx).me();
		let rows: Vec<AnyElement> = self
			.items
			.iter()
			.map(|item| self.row(item, me, &theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.member_list_width)
			.h_full()
			.bg(theme.background_secondary)
			.child(
				div()
					.px(theme.space_16)
					.pt(theme.space_24)
					.pb(theme.space_8)
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_secondary)
					.child(
						cx.t_args("group-members-title", &[("count", ArgValue::from(rows.len()))])
							.to_uppercase(),
					),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.children(rows)
					.overflow_y_scrollbar()
					.id("group-member-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
