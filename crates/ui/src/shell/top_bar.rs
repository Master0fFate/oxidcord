use std::time::Duration;

use gpui::{
	Animation, AnimationExt as _, AnyElement, AnyView, AppContext as _, ClickEvent, Context, Entity,
	EventEmitter, Focusable as _, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
	ParentElement as _, Render, SharedString, Styled as _, Window, deferred, div, ease_out_quint,
	prelude::FluentBuilder as _, px,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState};

use plugin_host::ViewContext;
use protocol::Snowflake;

use super::search_hints::{DateLabels, Leading, complete, pending, suggestions, with_filter};
use super::search_panel::Filter;
use super::transition::Pairs;
use crate::components::{Avatar, Divider, IconButton, StatusDot};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::model::Status;
use crate::theme::{Theme, rem};

const SEARCH_COLLAPSED: f32 = 220.;
const SEARCH_EXPANDED: f32 = 300.;
const SEARCH_SLIDE: Duration = Duration::from_millis(180);
const HISTORY_LIMIT: usize = 5;
const POPOVER_WIDTH: f32 = 360.;
const BASIC_FILTERS: [Filter; 4] = [Filter::From, Filter::In, Filter::Has, Filter::Mentions];
const MORE_FILTERS: [Filter; 5] = [
	Filter::Before,
	Filter::During,
	Filter::After,
	Filter::Pinned,
	Filter::AuthorType,
];

pub enum TopBarEvent {
	ToggleMembers,
	Search(String),
	Call { video: bool },
	Threads { at: gpui::Point<gpui::Pixels> },
	Pins { at: gpui::Point<gpui::Pixels> },
}

#[derive(Clone, PartialEq)]
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
	plugin_context: ViewContext,
	members_visible: bool,
	notifications_muted: bool,
	search: Entity<InputState>,
	search_focused: bool,
	search_width: Pairs<(), bool>,
	popover_closed: bool,
	more_filters: bool,
	history: Vec<SharedString>,
	group: Option<AnyView>,
}

impl EventEmitter<TopBarEvent> for TopBar {}

impl TopBar {
	pub fn new(title: TopBarTitle, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let placeholder = cx.t("top-bar-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		cx.subscribe_in(&search, window, |this, input, event: &InputEvent, _, cx| {
			let focused = match event {
				InputEvent::Focus => {
					this.popover_closed = false;
					true
				}
				InputEvent::Blur => false,
				InputEvent::Change => {
					this.popover_closed = false;
					cx.notify();
					return;
				}
				InputEvent::PressEnter { .. } => {
					let query = input.read(cx).value().trim().to_owned();
					this.run(query, cx);
					return;
				}
			};
			if this.search_focused != focused {
				this.search_focused = focused;
				cx.notify();
			}
		})
		.detach();
		Self {
			title,
			plugin_context: ViewContext::default(),
			members_visible: true,
			notifications_muted: false,
			search,
			search_focused: false,
			search_width: Pairs::default(),
			popover_closed: false,
			more_filters: false,
			history: Vec::new(),
			group: None,
		}
	}

	fn run(&mut self, query: String, cx: &mut Context<Self>) {
		if query.is_empty() {
			return;
		}
		self.history.retain(|previous| previous.as_ref() != query);
		self.history.insert(0, query.clone().into());
		self.history.truncate(HISTORY_LIMIT);
		self.popover_closed = true;
		cx.notify();
		cx.emit(TopBarEvent::Search(query));
	}

	pub fn search_in(&mut self, channel: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		self.set_query(format!("in:<#{channel}> "), window, cx);
	}

	fn set_query(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
		self.search
			.update(cx, |input, cx| input.set_value(text, window, cx));
		self.search.read(cx).focus_handle(cx).focus(window, cx);
		self.popover_closed = false;
		cx.notify();
	}

	pub fn set_group(&mut self, group: Option<AnyView>, cx: &mut Context<Self>) {
		if self.group.as_ref().map(AnyView::entity_id) != group.as_ref().map(AnyView::entity_id) {
			self.group = group;
			cx.notify();
		}
	}

	pub fn set_title(&mut self, title: TopBarTitle, cx: &mut Context<Self>) {
		if self.title != title {
			self.title = title;
			cx.notify();
		}
	}

	pub fn set_plugin_context(&mut self, context: ViewContext, cx: &mut Context<Self>) {
		if self.plugin_context != context {
			self.plugin_context = context;
			cx.notify();
		}
	}

	pub fn set_members_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
		if self.members_visible != visible {
			self.members_visible = visible;
			cx.notify();
		}
	}
}

fn filter_icon(filter: Filter) -> IconKind {
	match filter {
		Filter::From => IconKind::Account,
		Filter::In => IconKind::Hash,
		Filter::Has => IconKind::Attach,
		Filter::Mentions => IconKind::AtSign,
		Filter::Before | Filter::During | Filter::After => IconKind::Calendar,
		Filter::Pinned => IconKind::Pin,
		Filter::AuthorType => IconKind::Members,
	}
}

fn filter_hint(filter: Filter) -> &'static str {
	match filter {
		Filter::From | Filter::Mentions => "search-hint-user",
		Filter::In => "search-hint-channel",
		Filter::Has => "search-hint-has",
		Filter::Before | Filter::During | Filter::After => "search-hint-date",
		Filter::Pinned => "search-hint-pinned",
		Filter::AuthorType => "search-hint-authortype",
	}
}

fn section_title(title: SharedString, theme: &Theme) -> gpui::Div {
	div()
		.flex()
		.items_center()
		.justify_between()
		.px(theme.space_16)
		.pt(theme.space_8)
		.pb(theme.space_4)
		.text_size(theme.font_xs)
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(theme.header_secondary)
		.child(title)
}

fn icon_slot(icon: AnyElement) -> gpui::Div {
	div().flex().flex_none().w(rem(24.)).justify_center().child(icon)
}

fn row_leading(leading: &Leading, theme: &Theme) -> AnyElement {
	match leading {
		Leading::Icon(kind) => Icon::new(*kind)
			.size(rem(22.))
			.color(theme.interactive_normal)
			.into_any_element(),
		Leading::Avatar(url) => Avatar::new("").size(rem(24.)).url(url.clone()).into_any_element(),
	}
}

struct HintRow {
	id: gpui::ElementId,
	leading: AnyElement,
	title: SharedString,
	detail: Option<AnyElement>,
	inline: bool,
}

impl TopBar {
	fn hint_row(
		&self,
		row: HintRow,
		theme: &Theme,
		cx: &mut Context<Self>,
		pick: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
	) -> AnyElement {
		div()
			.id(row.id)
			.flex()
			.items_center()
			.gap(theme.space_12)
			.mx(theme.space_8)
			.px(theme.space_8)
			.py(rem(6.))
			.rounded(theme.radius_md)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_hover))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |this, _, window, cx| {
					cx.stop_propagation();
					pick(this, window, cx);
				}),
			)
			.child(icon_slot(row.leading))
			.child(
				div()
					.flex()
					.when(!row.inline, |text| text.flex_col())
					.when(row.inline, |text| text.items_baseline().gap(theme.space_8))
					.flex_1()
					.min_w_0()
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::MEDIUM)
							.text_color(theme.header_primary)
							.child(row.title),
					)
					.children(row.detail.map(|detail| {
						div()
							.truncate()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(detail)
					})),
			)
			.into_any_element()
	}

	fn filter_row(&self, filter: Filter, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let detail = div()
			.flex()
			.gap(theme.space_4)
			.child(
				div()
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_secondary)
					.child(format!("{}:", filter.key())),
			)
			.child(cx.t(filter_hint(filter)))
			.into_any_element();
		let row = HintRow {
			id: SharedString::from(format!("search-filter-{}", filter.key())).into(),
			leading: Icon::new(filter_icon(filter))
				.size(rem(22.))
				.color(theme.interactive_normal)
				.into_any_element(),
			title: cx.t(&format!("search-filter-{}", filter.key())),
			detail: Some(detail),
			inline: false,
		};
		self.hint_row(row, theme, cx, move |this, window, cx| {
			let text = with_filter(&this.search.read(cx).value(), filter);
			this.set_query(text, window, cx);
		})
	}

	fn history_row(
		&self,
		index: usize,
		query: SharedString,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let chip = div()
			.px(theme.space_8)
			.py(rem(2.))
			.rounded(theme.radius_sm)
			.bg(theme.button_secondary)
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.text_color(theme.header_primary)
			.truncate()
			.child(query.clone());
		div()
			.id(("search-history", index))
			.flex()
			.items_center()
			.gap(theme.space_12)
			.mx(theme.space_8)
			.px(theme.space_8)
			.py(rem(6.))
			.rounded(theme.radius_md)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_hover))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |this, _, window, cx| {
					cx.stop_propagation();
					this.search
						.update(cx, |input, cx| input.set_value(query.clone(), window, cx));
					this.run(query.to_string(), cx);
				}),
			)
			.child(icon_slot(
				Icon::new(IconKind::Search)
					.size(rem(20.))
					.color(theme.interactive_normal)
					.into_any_element(),
			))
			.child(div().flex().min_w_0().child(chip))
			.into_any_element()
	}

	fn filters_section(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		let mut out = vec![section_title(cx.t("search-filters"), theme).into_any_element()];
		for filter in BASIC_FILTERS {
			out.push(self.filter_row(filter, theme, cx));
		}
		if self.more_filters {
			for filter in MORE_FILTERS {
				out.push(self.filter_row(filter, theme, cx));
			}
		} else {
			let row = HintRow {
				id: "search-filter-more".into(),
				leading: Icon::new(IconKind::Funnel)
					.size(rem(22.))
					.color(theme.interactive_normal)
					.into_any_element(),
				title: cx.t("search-filter-more"),
				detail: Some(cx.t("search-filter-more-hint").into_any_element()),
				inline: false,
			};
			out.push(self.hint_row(row, theme, cx, |this, window, cx| {
				this.more_filters = true;
				this.search.read(cx).focus_handle(cx).focus(window, cx);
				cx.notify();
			}));
		}
		if self.history.is_empty() {
			return out;
		}
		out.push(
			div()
				.mx(theme.space_16)
				.my(theme.space_8)
				.h(rem(1.))
				.bg(theme.divider)
				.into_any_element(),
		);
		out.push(
			section_title(cx.t("search-history"), theme)
				.child(
					IconButton::new("search-clear-history", IconKind::Trash)
						.icon_size(rem(18.))
						.hit_size(rem(24.))
						.tooltip(cx.t("search-clear-history"))
						.on_click(cx.listener(|this, _, _, cx| {
							this.history.clear();
							cx.notify();
						})),
				)
				.into_any_element(),
		);
		for (index, query) in self.history.iter().enumerate() {
			out.push(self.history_row(index, query.clone(), theme, cx));
		}
		out
	}

	fn suggestion_section(
		&self,
		text: &str,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> Option<Vec<AnyElement>> {
		let current = pending(text)?;
		let labels = DateLabels {
			today: cx.t("search-today"),
			yesterday: cx.t("search-yesterday"),
		};
		let guild = self.plugin_context.guild_id.map(Snowflake::from);
		let channel = self.plugin_context.channel_id.map(Snowflake::from);
		let found = {
			let data = super::community::data_handle(cx);
			let data = data.read(cx);
			suggestions(&current, &data.state(), guild, channel, &labels)
		};
		let filter = current.filter;
		let mut out =
			vec![section_title(cx.t(&format!("search-filter-{}", filter.key())), theme).into_any_element()];
		if found.is_empty() {
			out.push(
				div()
					.px(theme.space_16)
					.py(theme.space_8)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t("search-no-matches"))
					.into_any_element(),
			);
		}
		for (index, suggestion) in found.into_iter().enumerate() {
			let value = suggestion.value;
			let row = HintRow {
				id: ("search-suggestion", index).into(),
				leading: row_leading(&suggestion.leading, theme),
				title: suggestion.title,
				detail: suggestion.detail.map(IntoElement::into_any_element),
				inline: true,
			};
			out.push(self.hint_row(row, theme, cx, move |this, window, cx| {
				let text = this.search.read(cx).value().to_string();
				if let Some(pending) = pending(&text) {
					let completed = complete(&text, &pending, &value);
					this.set_query(completed, window, cx);
				}
			}));
		}
		Some(out)
	}

	fn popover(&self, theme: &Theme, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
		let focused = self.search.read(cx).focus_handle(cx).is_focused(window);
		if !focused || self.popover_closed {
			return None;
		}
		let text = self.search.read(cx).value().to_string();
		let sections = match self.suggestion_section(&text, theme, cx) {
			Some(sections) => sections,
			None => self.filters_section(theme, cx),
		};
		let panel = div()
			.id("search-popover")
			.occlude()
			.absolute()
			.top(rem(38.))
			.right(rem(-8.))
			.w(rem(POPOVER_WIDTH))
			.flex()
			.flex_col()
			.py(theme.space_8)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.children(sections);
		Some(deferred(panel).with_priority(1).into_any_element())
	}
}

impl Render for TopBar {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
			_ if self.group.is_some() => div().flex().flex_1().min_w_0().children(self.group.clone()),
			TopBarTitle::Channel { name, icon, topic } => {
				let icon = *icon;
				div()
					.flex()
					.flex_1()
					.min_w_0()
					.items_center()
					.gap(theme.space_8)
					.child(Icon::new(icon).size(rem(22.)).color(theme.text_muted))
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
				.child(Icon::new(IconKind::AtSign).size(rem(22.)).color(theme.text_muted))
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
		let header_items = (!direct)
			.then(|| super::plugins::contextual("channel-header", self.plugin_context, cx))
			.filter(|items| !items.is_empty())
			.map(|items| {
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_8)
					.children(items)
			});
		let notifications = if self.notifications_muted {
			IconKind::BellOff
		} else {
			IconKind::Bell
		};
		let popover = self.popover(&theme, window, cx);
		let search_box = div()
			.relative()
			.flex()
			.flex_none()
			.items_center()
			.h(rem(30.))
			.pl(theme.space_4)
			.pr(theme.space_8)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
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
			.children(popover)
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
			.border_color(theme.divider)
			.child(leading)
			.children(header_items)
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
					.child(search_box),
			)
	}
}
