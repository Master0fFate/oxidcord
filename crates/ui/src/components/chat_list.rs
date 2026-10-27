use std::ops::Range;

use gpui::{
	AnyElement, App, Context, ElementId, Entity, FollowMode, InteractiveElement as _, IntoElement,
	ListAlignment, ListOffset, ListState, ParentElement as _, SharedString, Styled as _, Window, div, list,
	prelude::FluentBuilder as _, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{Icon, Sizable as _};

use crate::icons::IconKind;
use crate::theme::Theme;

const OVERDRAW: gpui::Pixels = px(400.);

pub struct ChatListState {
	list: ListState,
}

impl ChatListState {
	pub fn new(item_count: usize, cx: &mut Context<Self>) -> Self {
		let list = ListState::new(item_count, ListAlignment::Top, OVERDRAW);
		list.set_follow_mode(FollowMode::Tail);
		let weak = cx.weak_entity();
		list.set_scroll_handler(move |_, _, cx| {
			let weak = weak.clone();
			cx.defer(move |cx| {
				let _ = weak.update(cx, |_, cx| cx.notify());
			});
		});
		Self { list }
	}

	pub fn item_count(&self) -> usize {
		self.list.item_count()
	}

	pub fn is_following_tail(&self) -> bool {
		self.list.is_following_tail()
	}

	pub fn is_scrolled_up(&self) -> bool {
		!self.list.is_following_tail() && self.list.max_offset_for_scrollbar().y > px(0.)
	}

	pub fn logical_scroll_top(&self) -> ListOffset {
		self.list.logical_scroll_top()
	}

	pub fn reset(&mut self, item_count: usize, cx: &mut Context<Self>) {
		self.list.reset(item_count);
		self.list.set_follow_mode(FollowMode::Tail);
		cx.notify();
	}

	pub fn splice(&mut self, old_range: Range<usize>, count: usize, cx: &mut Context<Self>) {
		if old_range.start > old_range.end || old_range.end > self.list.item_count() {
			return;
		}
		self.list.splice(old_range, count);
		cx.notify();
	}

	pub fn remeasure(&mut self, cx: &mut Context<Self>) {
		self.list.remeasure();
		cx.notify();
	}

	pub fn remeasure_items(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
		if range.end > self.list.item_count() {
			return;
		}
		self.list.remeasure_items(range);
		cx.notify();
	}

	pub fn scroll_to_item(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
		if index >= self.list.item_count() {
			return false;
		}
		self.list.scroll_to(ListOffset {
			item_ix: index,
			offset_in_item: px(0.),
		});
		cx.notify();
		true
	}

	pub fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
		self.list.set_follow_mode(FollowMode::Tail);
		self.list.scroll_to_end();
		cx.notify();
	}
}

pub struct ChatList {
	id: ElementId,
	state: Entity<ChatListState>,
	jump_label: Option<SharedString>,
}

impl ChatList {
	pub fn new(id: impl Into<ElementId>, state: Entity<ChatListState>) -> Self {
		Self {
			id: id.into(),
			state,
			jump_label: None,
		}
	}

	pub fn jump_button(mut self, label: Option<SharedString>) -> Self {
		self.jump_label = label;
		self
	}

	pub fn render(
		self,
		render_row: impl Fn(usize, &mut Window, &mut App) -> AnyElement + 'static,
		cx: &App,
	) -> AnyElement {
		let theme = cx.global::<Theme>();
		let state = self.state.read(cx);
		let list_state = state.list.clone();
		let show_jump = self.jump_label.is_some() && state.is_scrolled_up();
		let jump_state = self.state.clone();
		let rows = list(list_state.clone(), move |index, window, cx| {
			div()
				.w_full()
				.min_w_0()
				.child(render_row(index, window, cx))
				.into_any_element()
		})
		.size_full()
		.min_h_0()
		.pt(theme.space_8)
		.pb(theme.space_16);
		div()
			.id(self.id)
			.relative()
			.size_full()
			.min_h_0()
			.overflow_hidden()
			.child(
				div()
					.size_full()
					.min_h_0()
					.min_w_0()
					.child(rows)
					.vertical_scrollbar(&list_state),
			)
			.when_some(self.jump_label.filter(|_| show_jump), |this, label| {
				this.child(
					div()
						.absolute()
						.left_0()
						.right_0()
						.bottom(theme.space_8)
						.flex()
						.justify_center()
						.child(
							Button::new("chat-list-jump")
								.secondary()
								.small()
								.icon(Icon::default().path(IconKind::ArrowDown.path()))
								.label(label)
								.rounded(theme.pixels(theme.radius_md))
								.on_click(move |_, _, cx| {
									jump_state.update(cx, |state, cx| state.scroll_to_end(cx));
								}),
						),
				)
			})
			.into_any_element()
	}
}
