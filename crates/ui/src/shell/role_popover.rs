use gpui::{
	ClickEvent, Context, Entity, EventEmitter, FontWeight, Hsla, InteractiveElement as _, IntoElement,
	ParentElement as _, Pixels, Point, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use state::RoleMembers;

use super::plugins;
use crate::components::Avatar;
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::model::role_display_color;
use crate::theme::{Theme, rem};

const AVATAR_PIXELS: u32 = 64;
const LIST_HEIGHT: f32 = 280.;

pub enum RolePopoverEvent {
	OpenProfile { user: Snowflake, at: Point<Pixels> },
	Close,
}

struct Entry {
	id: Snowflake,
	name: SharedString,
	color: Option<Hsla>,
	avatar: String,
}

struct Roster {
	name: SharedString,
	color: Option<Hsla>,
	count: usize,
	partial: bool,
	entries: Vec<Entry>,
}

pub struct RolePopover {
	data: Entity<Data>,
	guild: Snowflake,
	role: Snowflake,
	roster: Option<Roster>,
	_subscription: Subscription,
}

impl EventEmitter<RolePopoverEvent> for RolePopover {}

impl RolePopover {
	pub fn new(data: Entity<Data>, guild: Snowflake, role: Snowflake, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, move |this, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.members.contains(guild) || changes.loaded.contains(guild))
			{
				this.reload(cx);
			}
		});
		let mut popover = Self {
			data,
			guild,
			role,
			roster: None,
			_subscription: subscription,
		};
		popover.reload(cx);
		popover
	}

	fn reload(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		let roster = state.role_members(self.guild, self.role).map(|found| {
			let RoleMembers {
				name,
				color,
				members,
				count,
				partial,
			} = found;
			Roster {
				name: name.into(),
				color: color.map(role_display_color),
				count: usize::try_from(count).unwrap_or(usize::MAX),
				partial,
				entries: members
					.into_iter()
					.map(|member| Entry {
						id: member.user_id,
						name: member.display.into(),
						color: member.color.map(role_display_color),
						avatar: plugins::avatar_url(&state, Some(self.guild), member.user_id, AVATAR_PIXELS),
					})
					.collect(),
			}
		});
		drop(state);
		match roster {
			Some(roster) => self.roster = Some(roster),
			None => cx.emit(RolePopoverEvent::Close),
		}
		cx.notify();
	}
}

impl Render for RolePopover {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let Some(roster) = &self.roster else {
			return div().into_any_element();
		};
		let rows: Vec<_> = roster
			.entries
			.iter()
			.map(|entry| {
				let user = entry.id;
				div()
					.id(("role-member", user.get()))
					.flex()
					.items_center()
					.gap(theme.space_8)
					.px(theme.space_8)
					.h(theme.member_row_height)
					.rounded(theme.radius_md)
					.cursor_pointer()
					.hover(|style| style.bg(theme.modifier_hover))
					.on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
						cx.emit(RolePopoverEvent::OpenProfile {
							user,
							at: event.position(),
						})
					}))
					.child(
						Avatar::new(entry.name.clone())
							.size(rem(24.))
							.url(entry.avatar.clone()),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.text_color(entry.color.unwrap_or(theme.text_normal))
							.child(entry.name.clone()),
					)
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.w(theme.voice_menu_width)
			.p(theme.space_12)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						div()
							.size(theme.space_12)
							.flex_none()
							.rounded_full()
							.bg(roster.color.unwrap_or(theme.interactive_muted)),
					)
					.child(
						div()
							.flex_1()
							.truncate()
							.text_size(theme.font_sm)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(roster.name.clone()),
					)
					.child(
						div()
							.flex_none()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(cx.t_args("dm-group-members", &[("count", ArgValue::from(roster.count))])),
					),
			)
			.when(roster.partial, |card| {
				card.child(
					div()
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.child(cx.t("role-partial")),
				)
			})
			.child(
				div()
					.flex()
					.flex_col()
					.children(rows)
					.overflow_y_scrollbar()
					.id("role-member-list")
					.max_h(rem(LIST_HEIGHT)),
			)
			.into_any_element()
	}
}
