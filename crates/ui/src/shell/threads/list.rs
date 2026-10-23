use gpui::{
	AnyElement, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Channel, Permissions, Snowflake};
use session::Command;

use crate::components::{IconButton, PickHandler, segmented};
use crate::data::Data;
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

pub enum ThreadListEvent {
	Open(Snowflake),
	Create,
	Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
	Active,
	Archived,
}

struct Entry {
	id: Snowflake,
	name: SharedString,
	detail: SharedString,
	joined: bool,
	archived: bool,
}

pub struct ThreadList {
	data: Entity<Data>,
	guild: Snowflake,
	channel: Snowflake,
	tab: Tab,
}

impl EventEmitter<ThreadListEvent> for ThreadList {}

impl ThreadList {
	pub fn new(data: Entity<Data>, guild: Snowflake, channel: Snowflake, cx: &mut Context<Self>) -> Self {
		let list = Self {
			data,
			guild,
			channel,
			tab: Tab::Active,
		};
		list.reload(cx);
		list
	}

	fn reload(&self, cx: &Context<Self>) {
		let data = self.data.read(cx);
		data.send(Command::LoadActiveThreads { guild_id: self.guild });
		data.send(Command::LoadArchivedThreads {
			channel_id: self.channel,
		});
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		cx.notify();
	}

	fn entry(&self, thread: &Channel, state: &state::State, cx: &Context<Self>) -> Entry {
		let owner = thread
			.owner_id
			.map(|owner| state.display_name(Some(self.guild), owner))
			.unwrap_or_default();
		let count = thread.message_count.unwrap_or(0);
		let activity = thread
			.last_message_id
			.map_or_else(|| thread.id.timestamp(), Snowflake::timestamp);
		let when = format::message_time(activity, cx);
		let detail = cx.t_args(
			"thread-entry-detail",
			&[
				("owner", ArgValue::from(owner)),
				("count", ArgValue::from(count)),
				("when", ArgValue::from(when.to_string())),
			],
		);
		Entry {
			id: thread.id,
			name: thread.name.clone().unwrap_or_default().into(),
			detail,
			joined: thread.member.is_some(),
			archived: thread
				.thread_metadata
				.as_ref()
				.is_some_and(|metadata| metadata.archived),
		}
	}

	fn entries(&self, cx: &Context<Self>) -> Vec<Entry> {
		let data = self.data.read(cx);
		let state = data.state();
		let mut threads: Vec<&Channel> = match self.tab {
			Tab::Active => state
				.guild(self.guild)
				.map(|guild| {
					guild
						.threads
						.values()
						.filter(|thread| thread.parent_id == Some(self.channel))
						.filter(|thread| {
							thread
								.thread_metadata
								.as_ref()
								.is_none_or(|metadata| !metadata.archived)
						})
						.collect()
				})
				.unwrap_or_default(),
			Tab::Archived => state.archived_threads(self.channel),
		};
		threads.sort_by_key(|thread| std::cmp::Reverse(thread.last_message_id.unwrap_or(thread.id)));
		let entries = threads
			.into_iter()
			.map(|thread| self.entry(thread, &state, cx))
			.collect();
		drop(state);
		entries
	}

	fn can_create(&self, cx: &Context<Self>) -> bool {
		self.data
			.read(cx)
			.state()
			.my_permissions(self.channel)
			.contains(Permissions::CREATE_PUBLIC_THREADS)
	}

	fn row(&self, entry: &Entry, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = entry.id;
		let joined = entry.joined;
		let archived = entry.archived;
		div()
			.id(("thread-entry", id.get()))
			.flex()
			.items_center()
			.gap(theme.space_12)
			.mx(theme.space_8)
			.px(theme.space_12)
			.py(theme.space_8)
			.rounded(theme.radius_md)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_hover))
			.on_click(cx.listener(move |_, _, _, cx| cx.emit(ThreadListEvent::Open(id))))
			.child(
				Icon::new(if archived {
					IconKind::Archive
				} else {
					IconKind::Thread
				})
				.size(theme.glyph_lg)
				.color(theme.interactive_normal),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(entry.name.clone()),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(entry.detail.clone()),
					),
			)
			.child(
				Button::new(("thread-entry-membership", id.get()))
					.small()
					.outline()
					.label(cx.t(if joined { "thread-leave" } else { "thread-join" }))
					.on_click(cx.listener(move |this, _, _, cx| {
						let command = if joined {
							Command::LeaveThread { thread_id: id }
						} else {
							Command::JoinThread { thread_id: id }
						};
						this.data.read(cx).send(command);
					})),
			)
			.into_any_element()
	}
}

impl Render for ThreadList {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let entries = self.entries(cx);
		let empty = entries.is_empty();
		let rows: Vec<AnyElement> = entries.iter().map(|entry| self.row(entry, &theme, cx)).collect();
		let weak = cx.weak_entity();
		let tabs: PickHandler = std::rc::Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.tab = if index == 0 { Tab::Active } else { Tab::Archived };
				cx.notify();
			})
			.ok();
		});
		let labels = [cx.t("thread-tab-active"), cx.t("thread-tab-archived")];
		let selected = usize::from(self.tab == Tab::Archived);
		div()
			.flex()
			.flex_col()
			.w(theme.popover_width)
			.h(theme.popover_height)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.text_color(theme.text_normal)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_8)
					.h(theme.top_bar_height)
					.px(theme.space_16)
					.border_b_1()
					.border_color(theme.divider)
					.child(
						Icon::new(IconKind::Thread)
							.size(theme.glyph_lg)
							.color(theme.text_muted),
					)
					.child(
						div()
							.flex_1()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(cx.t("thread-list-title")),
					)
					.when(self.can_create(cx), |bar| {
						bar.child(
							Button::new("thread-create")
								.small()
								.primary()
								.label(cx.t("thread-create"))
								.on_click(cx.listener(|_, _, _, cx| cx.emit(ThreadListEvent::Create))),
						)
					})
					.child(
						IconButton::new("thread-list-close", IconKind::Close)
							.tooltip(cx.t("action-close"))
							.on_click(cx.listener(|_, _, _, cx| cx.emit(ThreadListEvent::Close))),
					),
			)
			.child(
				div()
					.flex_none()
					.px(theme.space_16)
					.py(theme.space_8)
					.child(segmented("thread-tabs", &labels, selected, &theme, tabs)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.children(rows)
					.when(empty, |list| {
						list.child(
							div()
								.flex()
								.flex_col()
								.items_center()
								.gap(theme.space_8)
								.p(theme.space_32)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(
									Icon::new(IconKind::Thread)
										.size(theme.portrait_lg)
										.color(theme.interactive_muted),
								)
								.child(cx.t("thread-empty")),
						)
					})
					.overflow_y_scrollbar()
					.id("thread-entries")
					.flex_1()
					.min_h_0(),
			)
	}
}
