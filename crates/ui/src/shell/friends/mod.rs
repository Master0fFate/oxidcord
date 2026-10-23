mod active_now;
mod add;
mod feature;
mod row;

use std::time::Duration;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, ParentElement as _, Pixels, Point, Render, SharedString, StatefulInteractiveElement as _,
	Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use state::Changes;
use state::RelationshipRow;

pub use self::feature::{FeatureKind, FeaturePage};
use crate::components::Divider;
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

const SENT_FALLBACK: Duration = Duration::from_millis(1500);

pub enum FriendsEvent {
	OpenDm,
	OpenProfile { user: Snowflake, at: Point<Pixels> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
	Online,
	All,
	Pending,
	Blocked,
	Add,
}

impl Tab {
	const LIST: [Tab; 4] = [Tab::Online, Tab::All, Tab::Pending, Tab::Blocked];

	fn key(self) -> &'static str {
		match self {
			Self::Online => "friends-tab-online",
			Self::All => "friends-tab-all",
			Self::Pending => "friends-tab-pending",
			Self::Blocked => "friends-tab-blocked",
			Self::Add => "friends-tab-add",
		}
	}
}

enum AddOutcome {
	Idle,
	Sending { baseline: u64, name: SharedString },
	Sent(SharedString),
	Failed(SharedString),
}

pub struct FriendsPage {
	data: Entity<Data>,
	tab: Tab,
	filter: String,
	search: Entity<InputState>,
	add_input: Entity<InputState>,
	friends: Vec<RelationshipRow>,
	incoming: Vec<RelationshipRow>,
	outgoing: Vec<RelationshipRow>,
	blocked: Vec<RelationshipRow>,
	add_outcome: AddOutcome,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<FriendsEvent> for FriendsPage {}

impl FriendsPage {
	pub fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let search_placeholder = cx.t("friends-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(search_placeholder));
		let add_placeholder = cx.t("friends-add-placeholder");
		let add_input = cx.new(|cx| InputState::new(window, cx).placeholder(add_placeholder));
		let subscriptions = vec![
			cx.subscribe_in(&search, window, |this, input, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					this.filter = input.read(cx).value().trim().to_lowercase();
					cx.notify();
				}
			}),
			cx.subscribe_in(&add_input, window, |this, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::PressEnter { .. }) {
					this.send_request(cx);
				}
			}),
		];
		let mut page = Self {
			data,
			tab: Tab::Online,
			filter: String::new(),
			search,
			add_input,
			friends: Vec::new(),
			incoming: Vec::new(),
			outgoing: Vec::new(),
			blocked: Vec::new(),
			add_outcome: AddOutcome::Idle,
			_subscriptions: subscriptions,
		};
		page.refresh(cx);
		page
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		self.friends = state.friends();
		self.incoming = state.pending_incoming();
		self.outgoing = state.pending_outgoing();
		self.blocked = state.blocked();
		drop(state);
		cx.notify();
	}

	pub fn on_changes(&mut self, changes: &Changes, cx: &mut Context<Self>) {
		if changes.relationships || !changes.presence.is_empty() {
			self.refresh(cx);
		}
		if changes.rejected {
			self.settle_rejection(cx);
		}
		if changes.relationships && matches!(self.add_outcome, AddOutcome::Sending { .. }) {
			self.finish_sending(cx);
		}
	}

	fn select(&mut self, tab: Tab, cx: &mut Context<Self>) {
		self.tab = tab;
		cx.notify();
	}

	fn matches(&self, row: &RelationshipRow) -> bool {
		self.filter.is_empty()
			|| row.display.to_lowercase().contains(&self.filter)
			|| row.username.to_lowercase().contains(&self.filter)
	}

	fn visible(&self, rows: &[RelationshipRow], online_only: bool) -> Vec<RelationshipRow> {
		rows.iter()
			.filter(|row| !online_only || is_online(&row.status))
			.filter(|row| self.matches(row))
			.cloned()
			.collect()
	}

	fn tab_button(&self, tab: Tab, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let selected = self.tab == tab;
		let count = (tab == Tab::Pending)
			.then_some(self.incoming.len())
			.filter(|n| *n > 0);
		let add = tab == Tab::Add;
		div()
			.id(tab.key())
			.flex()
			.items_center()
			.gap(theme.space_8)
			.h(theme.icon_lg + theme.space_4)
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_md)
			.font_weight(FontWeight::MEDIUM)
			.when(add && !selected, |button| {
				button.bg(theme.brand).text_color(theme.text_on_brand)
			})
			.when(add && selected, |button| button.text_color(theme.brand))
			.when(!add && selected, |button| {
				button
					.bg(theme.modifier_selected)
					.text_color(theme.interactive_active)
			})
			.when(!add && !selected, |button| {
				button
					.text_color(theme.interactive_normal)
					.hover(|style| style.bg(theme.modifier_hover).text_color(theme.interactive_hover))
			})
			.on_click(cx.listener(move |this, _, _, cx| this.select(tab, cx)))
			.child(cx.t(tab.key()))
			.children(count.map(|count| crate::components::MentionBadge::new(count as u32)))
			.into_any_element()
	}

	fn header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_16)
			.h(theme.top_bar_height)
			.px(theme.space_16)
			.border_b_1()
			.border_color(theme.background_tertiary)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.text_size(theme.font_md)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(
						Icon::new(IconKind::Friends)
							.size(theme.icon_lg)
							.color(theme.text_muted),
					)
					.child(cx.t("friends-title")),
			)
			.child(Divider::vertical().length(theme.icon_md).color(theme.divider))
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.children(Tab::LIST.map(|tab| self.tab_button(tab, theme, cx)))
					.child(self.tab_button(Tab::Add, theme, cx)),
			)
			.into_any_element()
	}

	fn section_title(&self, key: &str, count: usize, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		div()
			.px(theme.space_8)
			.pt(theme.space_16)
			.pb(theme.space_8)
			.text_size(theme.font_tiny)
			.font_weight(FontWeight::BOLD)
			.text_color(theme.header_secondary)
			.child(cx.t_args(key, &[("count", ArgValue::from(count))]).to_uppercase())
			.into_any_element()
	}

	fn empty(&self, key: &str, icon: IconKind, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		div()
			.flex()
			.flex_col()
			.flex_1()
			.items_center()
			.justify_center()
			.gap(theme.space_16)
			.text_size(theme.font_md)
			.text_color(theme.text_muted)
			.child(
				Icon::new(icon)
					.size(theme.avatar_lg)
					.color(theme.interactive_muted),
			)
			.child(cx.t(key))
			.into_any_element()
	}

	fn list_body(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let sections: Vec<(&str, Vec<RelationshipRow>, row::Kind)> = match self.tab {
			Tab::Online => vec![(
				"friends-online-count",
				self.visible(&self.friends, true),
				row::Kind::Friend,
			)],
			Tab::All => vec![(
				"friends-all-count",
				self.visible(&self.friends, false),
				row::Kind::Friend,
			)],
			Tab::Pending => vec![
				(
					"friends-incoming-count",
					self.visible(&self.incoming, false),
					row::Kind::Incoming,
				),
				(
					"friends-outgoing-count",
					self.visible(&self.outgoing, false),
					row::Kind::Outgoing,
				),
			],
			Tab::Blocked => vec![(
				"friends-blocked-count",
				self.visible(&self.blocked, false),
				row::Kind::Blocked,
			)],
			Tab::Add => Vec::new(),
		};
		let empty_key = match self.tab {
			Tab::Online => "friends-empty-online",
			Tab::All => "friends-empty-all",
			Tab::Pending => "friends-empty-pending",
			_ => "friends-empty-blocked",
		};
		let any = sections.iter().any(|(_, rows, _)| !rows.is_empty());
		if !any {
			return self.empty(empty_key, IconKind::Friends, theme, cx);
		}
		let mut children: Vec<AnyElement> = Vec::new();
		for (title, rows, kind) in sections {
			if rows.is_empty() {
				continue;
			}
			children.push(self.section_title(title, rows.len(), theme, cx));
			for item in rows {
				children.push(self.row(&item, kind, theme, cx));
			}
		}
		div()
			.flex()
			.flex_col()
			.children(children)
			.overflow_y_scrollbar()
			.id("friends-list")
			.flex_1()
			.min_h_0()
			.into_any_element()
	}

	fn search_box(&self, theme: &Theme) -> AnyElement {
		div()
			.flex_none()
			.p(theme.space_16)
			.child(Input::new(&self.search).large().cleanable(true))
			.into_any_element()
	}
}

impl Render for FriendsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let header = self.header(&theme, cx);
		let main = if self.tab == Tab::Add {
			self.add_form(&theme, cx)
		} else {
			div()
				.flex()
				.flex_col()
				.flex_1()
				.min_h_0()
				.child(self.search_box(&theme))
				.child(
					div()
						.flex()
						.flex_col()
						.flex_1()
						.min_h_0()
						.px(theme.space_16)
						.child(self.list_body(&theme, cx)),
				)
				.into_any_element()
		};
		let active = self.active_now(&theme, cx);
		div()
			.flex()
			.flex_col()
			.size_full()
			.bg(theme.background_primary)
			.child(header)
			.child(
				div()
					.flex()
					.flex_1()
					.min_h_0()
					.child(div().flex().flex_col().flex_1().min_w_0().child(main))
					.child(active),
			)
	}
}

fn is_online(status: &protocol::StatusKind) -> bool {
	!matches!(
		status,
		protocol::StatusKind::Offline | protocol::StatusKind::Invisible
	)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomePage {
	Friends,
	Nitro,
	Shop,
}

pub struct HomePages {
	pub friends: Entity<FriendsPage>,
	feature: Entity<FeaturePage>,
	current: Option<HomePage>,
}

impl HomePages {
	pub fn new<T: 'static>(data: Entity<Data>, window: &mut Window, cx: &mut Context<T>) -> Self {
		Self {
			friends: cx.new(|cx| FriendsPage::new(data, window, cx)),
			feature: cx.new(|_| FeaturePage::new(FeatureKind::Nitro)),
			current: None,
		}
	}

	pub fn show<T: 'static>(&mut self, page: HomePage, cx: &mut Context<T>) {
		match page {
			HomePage::Nitro => self
				.feature
				.update(cx, |feature, cx| feature.show(FeatureKind::Nitro, cx)),
			HomePage::Shop => self
				.feature
				.update(cx, |feature, cx| feature.show(FeatureKind::Shop, cx)),
			HomePage::Friends => {}
		}
		self.current = Some(page);
		cx.notify();
	}

	pub fn close<T: 'static>(&mut self, cx: &mut Context<T>) {
		if self.current.take().is_some() {
			cx.notify();
		}
	}

	pub fn on_changes<T: 'static>(&mut self, changes: &Changes, cx: &mut Context<T>) {
		self.friends
			.update(cx, |friends, cx| friends.on_changes(changes, cx));
	}

	pub fn element(&self, theme: &Theme) -> Option<AnyElement> {
		let page = match self.current? {
			HomePage::Friends => self.friends.clone().into_any_element(),
			HomePage::Nitro | HomePage::Shop => self.feature.clone().into_any_element(),
		};
		Some(
			div()
				.absolute()
				.top_0()
				.left_0()
				.size_full()
				.occlude()
				.bg(theme.background_primary)
				.child(page)
				.into_any_element(),
		)
	}
}
