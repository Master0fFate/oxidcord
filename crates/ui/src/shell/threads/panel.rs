use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use protocol::{Permissions, Snowflake};
use session::Command;

use crate::components::IconButton;
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::composer::Composer;
use crate::shell::timeline::Timeline;
use crate::theme::Theme;

pub enum ThreadPanelEvent {
	Close,
}

pub struct ThreadPanel {
	data: Entity<Data>,
	timeline: Entity<Timeline>,
	composer: Entity<Composer>,
	thread: Option<Snowflake>,
}

impl EventEmitter<ThreadPanelEvent> for ThreadPanel {}

struct Header {
	name: SharedString,
	members: i64,
	archived: bool,
	joined: bool,
	can_manage: bool,
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

	pub fn show(
		&mut self,
		guild: Option<Snowflake>,
		thread: Option<Snowflake>,
		compact: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.thread = thread;
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

	fn header_state(&self, cx: &Context<Self>) -> Option<Header> {
		let thread = self.thread?;
		let data = self.data.read(cx);
		let state = data.state();
		let channel = state.channel(thread)?;
		let me = state.current_user_id();
		let manage = state.my_permissions(thread).contains(Permissions::MANAGE_THREADS);
		Some(Header {
			name: channel.name.clone().unwrap_or_default().into(),
			members: channel.member_count.unwrap_or(0),
			archived: channel
				.thread_metadata
				.as_ref()
				.is_some_and(|metadata| metadata.archived),
			joined: channel.member.is_some(),
			can_manage: manage || (me.is_some() && channel.owner_id == me),
		})
	}

	fn send(&self, command: Command, cx: &Context<Self>) {
		self.data.read(cx).send(command);
	}

	fn header(&self, header: &Header, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let Some(thread) = self.thread else {
			return div().into_any_element();
		};
		let archived = header.archived;
		let joined = header.joined;
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
					.min_w_0()
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(header.name.clone()),
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
			.when(header.can_manage, |bar| {
				bar.child(
					IconButton::new(
						"thread-archive",
						if archived {
							IconKind::Unarchive
						} else {
							IconKind::Archive
						},
					)
					.tooltip(cx.t(if archived {
						"thread-unarchive"
					} else {
						"thread-archive"
					}))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.send(
							Command::ArchiveThread {
								thread_id: thread,
								archived: !archived,
							},
							cx,
						)
					})),
				)
			})
			.child(
				IconButton::new(
					"thread-membership",
					if joined { IconKind::LogOut } else { IconKind::Join },
				)
				.tooltip(cx.t(if joined { "thread-leave" } else { "thread-join" }))
				.on_click(cx.listener(move |this, _, _, cx| {
					let command = if joined {
						Command::LeaveThread { thread_id: thread }
					} else {
						Command::JoinThread { thread_id: thread }
					};
					this.send(command, cx);
				})),
			)
			.child(
				IconButton::new("thread-close", IconKind::Close)
					.tooltip(cx.t("thread-close"))
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
		if header.archived {
			return Some(
				bar.child(div().flex_1().min_w_0().child(cx.t("thread-archived-notice")))
					.when(header.can_manage, |bar| {
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
		if header.joined {
			return None;
		}
		Some(
			bar.child(div().flex_1().min_w_0().truncate().child(cx.t_args(
				"thread-join-notice",
				&[("name", ArgValue::from(header.name.to_string()))],
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
}

impl Render for ThreadPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let header = self.header_state(cx);
		let archived = header.as_ref().is_some_and(|header| header.archived);
		let top = header.as_ref().map(|header| self.header(header, &theme, cx));
		let notice = header.as_ref().and_then(|header| self.notice(header, &theme, cx));
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.thread_panel_width)
			.h_full()
			.min_h_0()
			.bg(theme.background_primary)
			.border_l_1()
			.border_color(theme.divider)
			.children(top)
			.child(self.timeline.clone())
			.children(notice)
			.when(!archived, |panel| panel.child(self.composer.clone()))
	}
}
