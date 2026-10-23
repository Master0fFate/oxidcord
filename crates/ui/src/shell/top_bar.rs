use std::time::Duration;

use gpui::{
	Animation, AnimationExt as _, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FontWeight,
	IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div, ease_out_quint,
	prelude::FluentBuilder as _, px,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState};

use super::transition::Pairs;
use crate::components::{Divider, IconButton, StatusDot};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::model::Status;
use crate::theme::{Theme, rem};

const SEARCH_COLLAPSED: f32 = 144.;
const SEARCH_EXPANDED: f32 = 240.;
const SEARCH_SLIDE: Duration = Duration::from_millis(180);

pub enum TopBarEvent {
	ToggleMembers,
	Search(String),
	Call { video: bool },
	Threads { at: gpui::Point<gpui::Pixels> },
	Pins { at: gpui::Point<gpui::Pixels> },
}

#[derive(Clone)]
pub enum TopBarTitle {
	Channel {
		name: SharedString,
		icon: IconKind,
		topic: Option<SharedString>,
	},
	Direct {
		name: SharedString,
		status: Status,
	},
}

pub struct TopBar {
	title: TopBarTitle,
	members_visible: bool,
	notifications_muted: bool,
	search: Entity<InputState>,
	search_focused: bool,
	search_width: Pairs<(), bool>,
}

impl EventEmitter<TopBarEvent> for TopBar {}

impl TopBar {
	pub fn new(title: TopBarTitle, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let placeholder = cx.t("top-bar-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		cx.subscribe_in(&search, window, |this, input, event: &InputEvent, _, cx| {
			let focused = match event {
				InputEvent::Focus => true,
				InputEvent::Blur => false,
				InputEvent::PressEnter { .. } => {
					let query = input.read(cx).value().trim().to_owned();
					if !query.is_empty() {
						cx.emit(TopBarEvent::Search(query));
					}
					return;
				}
				_ => return,
			};
			if this.search_focused != focused {
				this.search_focused = focused;
				cx.notify();
			}
		})
		.detach();
		Self {
			title,
			members_visible: true,
			notifications_muted: false,
			search,
			search_focused: false,
			search_width: Pairs::default(),
		}
	}

	pub fn set_title(&mut self, title: TopBarTitle, cx: &mut Context<Self>) {
		self.title = title;
		cx.notify();
	}

	pub fn set_members_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
		if self.members_visible != visible {
			self.members_visible = visible;
			cx.notify();
		}
	}
}

impl Render for TopBar {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (from, to) = self.search_width.settle((), self.search_focused);
		let width = |expanded: bool| {
			if expanded {
				SEARCH_EXPANDED
			} else {
				SEARCH_COLLAPSED
			}
		};
		let (from_width, to_width) = (width(from), width(to));
		let direct = matches!(self.title, TopBarTitle::Direct { .. });
		let leading = match &self.title {
			TopBarTitle::Channel { name, icon, topic } => {
				let icon = *icon;
				div()
					.flex()
					.flex_1()
					.min_w_0()
					.items_center()
					.gap(theme.space_8)
					.child(Icon::new(icon).size(rem(24.)).color(theme.text_muted))
					.child(
						div()
							.flex_none()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(name.clone()),
					)
					.when_some(topic.clone(), |bar, topic| {
						bar.child(
							Divider::vertical()
								.length(rem(24.))
								.thickness(px(1.))
								.color(theme.divider),
						)
						.child(
							div()
								.flex_1()
								.min_w_0()
								.truncate()
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(topic),
						)
					})
			}
			TopBarTitle::Direct { name, status } => div()
				.flex()
				.flex_1()
				.min_w_0()
				.items_center()
				.gap(theme.space_8)
				.child(Icon::new(IconKind::AtSign).size(rem(24.)).color(theme.text_muted))
				.child(
					div()
						.flex_none()
						.text_size(theme.font_md)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(theme.header_primary)
						.child(name.clone()),
				)
				.child(
					StatusDot::new(*status)
						.size(rem(10.))
						.cutout(theme.background_primary),
				),
		};
		let notifications = if self.notifications_muted {
			IconKind::BellOff
		} else {
			IconKind::Bell
		};
		let search_box = div()
			.flex()
			.flex_none()
			.items_center()
			.h(rem(24.))
			.pl(theme.space_4)
			.pr(theme.space_8)
			.rounded(theme.radius_sm)
			.bg(theme.background_tertiary)
			.w(rem(to_width))
			.child(
				Input::new(&self.search)
					.appearance(false)
					.bordered(false)
					.focus_bordered(false)
					.small()
					.w_full(),
			)
			.child(Icon::new(IconKind::Search).size(rem(16.)).color(theme.text_muted))
			.with_animation(
				("top-bar-search", u64::from(to)),
				Animation::new(SEARCH_SLIDE).with_easing(ease_out_quint()),
				move |search, delta| search.w(rem(from_width + (to_width - from_width) * delta)),
			);
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_8)
			.h(theme.top_bar_height)
			.px(theme.space_16)
			.bg(theme.background_primary)
			.border_b_1()
			.border_color(theme.background_tertiary)
			.child(leading)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_16)
					.when(!direct, |bar| {
						bar.child(
							IconButton::new("top-threads", IconKind::Thread)
								.tooltip(cx.t("top-bar-threads"))
								.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
									cx.emit(TopBarEvent::Threads { at: event.position() })
								})),
						)
					})
					.when(direct, |bar| {
						bar.child(
							IconButton::new("top-call", IconKind::Phone)
								.tooltip(cx.t("top-bar-call"))
								.on_click(
									cx.listener(|_, _, _, cx| cx.emit(TopBarEvent::Call { video: false })),
								),
						)
						.child(
							IconButton::new("top-video", IconKind::Video)
								.tooltip(cx.t("top-bar-video"))
								.on_click(
									cx.listener(|_, _, _, cx| cx.emit(TopBarEvent::Call { video: true })),
								),
						)
					})
					.when(!direct, |bar| {
						bar.child(
							IconButton::new("top-notifications", notifications)
								.tooltip(cx.t("top-bar-notifications"))
								.on_click(cx.listener(|this, _, _, cx| {
									this.notifications_muted = !this.notifications_muted;
									cx.notify();
								})),
						)
					})
					.child(
						IconButton::new("top-pinned", IconKind::Pin)
							.tooltip(cx.t("top-bar-pinned"))
							.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
								cx.emit(TopBarEvent::Pins { at: event.position() })
							})),
					)
					.when(!direct, |bar| {
						bar.child(
							IconButton::new("top-members", IconKind::Members)
								.tooltip(cx.t(if self.members_visible {
									"top-bar-hide-members"
								} else {
									"top-bar-show-members"
								}))
								.active(self.members_visible)
								.on_click(cx.listener(|_, _, _, cx| cx.emit(TopBarEvent::ToggleMembers))),
						)
					})
					.children(super::plugins::panel_toggle(cx))
					.child(search_box)
					.child(IconButton::new("top-inbox", IconKind::Inbox).tooltip(cx.t("top-bar-inbox")))
					.child(IconButton::new("top-help", IconKind::Help).tooltip(cx.t("top-bar-help"))),
			)
	}
}
