use gpui::{
	AnyElement, AppContext as _, Context, DragMoveEvent, Empty, Entity, EventEmitter, FontWeight,
	InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::DropdownMenu as _;
use protocol::{ChannelType, Permissions, Snowflake};
use session::Command;

use super::menu::{ThreadFacts, thread_menu};
use crate::components::IconButton;
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::composer::{Composer, accept_files};
use crate::shell::timeline::Timeline;
use crate::theme::{Theme, rem, rem_to_pixels};

const MIN_WIDTH: f32 = 340.;
const MAIN_MIN_WIDTH: f32 = 400.;
const HANDLE_WIDTH: f32 = 6.;
const BUTTON_SIZE: f32 = 28.;

pub enum ThreadPanelEvent {
	Close,
	Layout,
}

struct ResizeHandle;

impl Render for ResizeHandle {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		Empty
	}
}

pub struct ThreadPanel {
	data: Entity<Data>,
	timeline: Entity<Timeline>,
	composer: Entity<Composer>,
	thread: Option<Snowflake>,
	width: Option<f32>,
	full: bool,
}

impl EventEmitter<ThreadPanelEvent> for ThreadPanel {}

struct Header {
	facts: ThreadFacts,
	members: i64,
}

impl ThreadPanel {
	pub fn new(data: Entity<Data>, compact: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let timeline = cx.new(|cx| Timeline::new(data.clone(), compact, cx));
		let composer = cx.new(|cx| Composer::new(data.clone(), window, cx));
		Self {
			data,
			timeline,
			composer,
			thread: None,
			width: None,
			full: false,
		}
	}

	pub fn timeline(&self) -> &Entity<Timeline> {
		&self.timeline
	}

	pub fn composer(&self) -> &Entity<Composer> {
		&self.composer
	}

	pub fn thread(&self) -> Option<Snowflake> {
		self.thread
	}

	pub fn is_full(&self) -> bool {
		self.full
	}

	pub fn show(
		&mut self,
		guild: Option<Snowflake>,
		thread: Option<Snowflake>,
		compact: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.thread = thread;
		if thread.is_none() {
			self.full = false;
		}
		let name: SharedString = thread
			.and_then(|id| {
				self.data
					.read(cx)
					.state()
					.channel(id)
					.and_then(|channel| channel.name.clone())
			})
			.unwrap_or_default()
			.into();
		self.timeline.update(cx, |timeline, cx| {
			timeline.set_compact(compact, cx);
			timeline.show(guild, thread, name.clone(), false, window, cx);
		});
		self.composer.update(cx, |composer, cx| {
			composer.show(guild, thread, name, false, window, cx)
		});
		cx.notify();
	}

	pub fn toggle_full(&mut self, cx: &mut Context<Self>) {
		self.full = !self.full;
		cx.emit(ThreadPanelEvent::Layout);
		cx.notify();
	}

	fn resize(&mut self, event: &DragMoveEvent<ResizeHandle>, window: &mut Window, cx: &mut Context<Self>) {
		let unit = rem(1.).to_pixels(window.rem_size());
		let wanted = (event.bounds.right() - event.event.position.x) / unit;
		let theme = cx.global::<Theme>();
		let reserved = rem_to_pixels(theme.server_rail_width)
			+ rem_to_pixels(theme.channel_sidebar_width)
			+ MAIN_MIN_WIDTH;
		let widest = (window.viewport_size().width / unit - reserved).max(MIN_WIDTH);
		self.width = Some(wanted.clamp(MIN_WIDTH, widest));
		cx.notify();
	}

	fn header_state(&self, cx: &Context<Self>) -> Option<Header> {
		let thread = self.thread?;
		let data = self.data.read(cx);
		let state = data.state();
		let channel = state.channel(thread)?;
		let me = state.current_user_id();
		let moderate = state.my_permissions(thread).contains(Permissions::MANAGE_THREADS);
		let metadata = channel.thread_metadata.as_ref();
		let post = channel
			.parent_id
			.and_then(|parent| state.channel(parent))
			.is_some_and(|parent| matches!(parent.kind, ChannelType::GuildForum | ChannelType::GuildMedia));
		Some(Header {
			facts: ThreadFacts {
				thread,
				guild: state.channel_guild(thread),
				name: channel.name.clone().unwrap_or_default(),
				post,
				joined: channel.member.is_some(),
				muted: state.thread_muted(thread),
				archived: metadata.is_some_and(|metadata| metadata.archived),
				locked: metadata.is_some_and(|metadata| metadata.locked),
				can_edit: moderate || (me.is_some() && channel.owner_id == me),
				can_moderate: moderate,
				full: self.full,
			},
			members: channel.member_count.unwrap_or(0),
		})
	}

	fn send(&self, command: Command, cx: &Context<Self>) {
		self.data.read(cx).send(command);
	}

	fn header(&self, header: &Header, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let weak = cx.weak_entity();
		let menu = thread_menu(header.facts.clone(), move |_, cx| {
			weak.update(cx, |panel, cx| panel.toggle_full(cx)).ok();
		});
		let icon = if header.facts.post {
			IconKind::Forum
		} else {
			IconKind::Thread
		};
		let (view_icon, view_tip) = if self.full {
			(IconKind::Shrink, "thread-close-full-view")
		} else {
			(IconKind::Expand, "thread-open-full-view")
		};
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_8)
			.h(theme.top_bar_height)
			.px(theme.space_16)
			.border_b_1()
			.border_color(theme.divider)
			.child(Icon::new(icon).size(theme.glyph_lg).color(theme.text_muted))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(header.facts.name.clone()),
			)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_4)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(
						Icon::new(IconKind::Members)
							.size(theme.glyph_sm)
							.color(theme.text_muted),
					)
					.child(header.members.to_string()),
			)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.justify_center()
					.size(rem(BUTTON_SIZE))
					.child(
						Button::new("thread-more")
							.ghost()
							.size_full()
							.tooltip(cx.t("message-more"))
							.icon(gpui_component::Icon::default().path(IconKind::More.path()))
							.dropdown_menu_with_anchor(gpui::Anchor::TopRight, menu.build()),
					),
			)
			.child(
				IconButton::new("thread-full-view", view_icon)
					.tooltip(cx.t(view_tip))
					.on_click(cx.listener(|this, _, _, cx| this.toggle_full(cx))),
			)
			.child(
				IconButton::new("thread-close", IconKind::Close)
					.tooltip(cx.t("action-close"))
					.on_click(cx.listener(|_, _, _, cx| cx.emit(ThreadPanelEvent::Close))),
			)
			.into_any_element()
	}

	fn notice(&self, header: &Header, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		let thread = self.thread?;
		let bar = div()
			.flex()
			.flex_none()
			.items_center()
			.justify_between()
			.gap(theme.space_8)
			.mx(theme.space_16)
			.mb(theme.space_16)
			.px(theme.space_12)
			.py(theme.space_8)
			.rounded(theme.radius_md)
			.bg(theme.background_secondary)
			.text_size(theme.font_sm)
			.text_color(theme.text_muted);
		if header.facts.archived {
			return Some(
				bar.child(div().flex_1().min_w_0().child(cx.t("thread-archived-notice")))
					.when(header.facts.can_edit, |bar| {
						bar.child(
							Button::new("thread-unarchive-notice")
								.small()
								.primary()
								.label(cx.t("thread-unarchive"))
								.on_click(cx.listener(move |this, _, _, cx| {
									this.send(
										Command::ArchiveThread {
											thread_id: thread,
											archived: false,
										},
										cx,
									)
								})),
						)
					})
					.into_any_element(),
			);
		}
		if header.facts.joined {
			return None;
		}
		Some(
			bar.child(div().flex_1().min_w_0().truncate().child(cx.t_args(
				"thread-join-notice",
				&[("name", ArgValue::from(header.facts.name.clone()))],
			)))
			.child(
				Button::new("thread-join-notice")
					.small()
					.primary()
					.label(cx.t("thread-join"))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.send(Command::JoinThread { thread_id: thread }, cx)
					})),
			)
			.into_any_element(),
		)
	}

	fn resize_handle(&self, theme: &Theme) -> AnyElement {
		div()
			.id("thread-resize")
			.absolute()
			.top_0()
			.bottom_0()
			.left(px(-HANDLE_WIDTH / 2.))
			.w(px(HANDLE_WIDTH))
			.cursor_col_resize()
			.hover(|style| style.bg(theme.divider))
			.on_drag(ResizeHandle, |_, _, _, cx| cx.new(|_| ResizeHandle))
			.into_any_element()
	}
}

impl Render for ThreadPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let header = self.header_state(cx);
		let archived = header.as_ref().is_some_and(|header| header.facts.archived);
		let top = header.as_ref().map(|header| self.header(header, &theme, cx));
		let notice = header.as_ref().and_then(|header| self.notice(header, &theme, cx));
		let width = self.width.map_or(theme.thread_panel_width, rem);
		div()
			.relative()
			.flex()
			.flex_col()
			.h_full()
			.min_h_0()
			.bg(theme.background_primary)
			.map(|panel| {
				if self.full {
					panel.flex_1().min_w_0()
				} else {
					panel
						.flex_none()
						.w(width)
						.border_l_1()
						.border_color(theme.divider)
						.on_drag_move(cx.listener(Self::resize))
						.child(self.resize_handle(&theme))
				}
			})
			.children(top)
			.child(self.timeline.clone())
			.children(notice)
			.when(!archived, |panel| {
				accept_files(panel, self.composer.clone()).child(self.composer.clone())
			})
	}
}
