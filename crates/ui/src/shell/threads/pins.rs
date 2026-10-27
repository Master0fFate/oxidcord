use gpui::{
	AnyElement, App, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render,
	Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Message, Snowflake};
use session::Command;
use std::sync::Arc;

use crate::components::{Avatar, IconButton};
use crate::data::Data;
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::lookup::user_avatar;
use crate::shell::message::snippet;
use crate::theme::Theme;

const SNIPPET_LENGTH: usize = 260;

pub enum PinsEvent {
	Jump { channel: Snowflake, message: Snowflake },
	Close,
}

pub struct PinsPanel {
	data: Entity<Data>,
	channel: Snowflake,
}

impl EventEmitter<PinsEvent> for PinsPanel {}

pub fn confirm(
	window: &mut Window,
	cx: &mut App,
	data: Entity<Data>,
	channel: Snowflake,
	message: Snowflake,
	pin: bool,
) {
	let (title, body, ok) = if pin {
		("pin-title", "pin-body", "pin-confirm")
	} else {
		("unpin-title", "unpin-body", "unpin-confirm")
	};
	window.open_alert_dialog(cx, move |alert, _, cx| {
		let data = data.clone();
		alert
			.title(cx.t(title))
			.description(cx.t(body))
			.confirm()
			.ok_text(cx.t(ok))
			.ok_variant(if pin {
				ButtonVariant::Primary
			} else {
				ButtonVariant::Danger
			})
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				let command = if pin {
					Command::PinMessage {
						channel_id: channel,
						message_id: message,
					}
				} else {
					Command::UnpinMessage {
						channel_id: channel,
						message_id: message,
					}
				};
				data.read(cx).send(command);
				true
			})
	});
}

impl PinsPanel {
	pub fn new(data: Entity<Data>, channel: Snowflake, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadPins { channel_id: channel });
		Self { data, channel }
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		cx.notify();
	}

	fn card(
		&self,
		message: &Arc<Message>,
		can_manage: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = message.id;
		let channel = self.channel;
		let data = self.data.read(cx);
		let state = data.state();
		let guild = state.channel_guild(channel);
		let name = state.display_name(guild, message.author.id);
		drop(state);
		let text = if message.content.is_empty() {
			cx.t("message-reply-attachment").to_string()
		} else {
			snippet(&markdown::parse(&message.content), SNIPPET_LENGTH)
		};
		let when = format::message_time(message.timestamp.unwrap_or_else(|| id.timestamp()), cx);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.mx(theme.space_8)
			.mb(theme.space_8)
			.p(theme.space_12)
			.rounded(theme.radius_md)
			.bg(theme.background_primary)
			.border_1()
			.border_color(theme.divider)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						Avatar::new(name.clone())
							.size(theme.portrait_md)
							.url(user_avatar(&message.author, 64)),
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
									.text_size(theme.font_sm)
									.font_weight(FontWeight::SEMIBOLD)
									.text_color(theme.header_primary)
									.child(name),
							)
							.child(
								div()
									.text_size(theme.font_tiny)
									.text_color(theme.text_muted)
									.child(when),
							),
					)
					.child(
						Button::new(("pin-jump", id.get()))
							.small()
							.outline()
							.label(cx.t("pin-jump"))
							.on_click(cx.listener(move |_, _, _, cx| {
								cx.emit(PinsEvent::Jump { channel, message: id })
							})),
					)
					.when(can_manage, |row| {
						row.child(
							IconButton::new(("pin-remove", id.get()), IconKind::Unpin)
								.tooltip(cx.t("menu-unpin"))
								.on_click(cx.listener(move |this, _, window, cx| {
									confirm(window, cx, this.data.clone(), this.channel, id, false)
								})),
						)
					}),
			)
			.child(
				div()
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.line_clamp(5)
					.child(text),
			)
			.into_any_element()
	}
}

impl Render for PinsPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (messages, can_manage, count) = {
			let data = self.data.read(cx);
			let state = data.state();
			let messages: Vec<Arc<Message>> = state
				.pins(self.channel)
				.map(|pins| pins.iter().rev().cloned().collect())
				.unwrap_or_default();
			let count = messages.len();
			(messages, state.can_pin(self.channel), count)
		};
		let loaded = self.data.read(cx).state().pins(self.channel).is_some();
		let cards: Vec<AnyElement> = messages
			.iter()
			.map(|message| self.card(message, can_manage, &theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.w(theme.popover_width)
			.h(theme.popover_height)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_secondary)
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
						Icon::new(IconKind::Pin)
							.size(theme.glyph_lg)
							.color(theme.text_muted),
					)
					.child(
						div()
							.flex_1()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(cx.t_args("pins-title", &[("count", ArgValue::from(count))])),
					)
					.child(
						IconButton::new("pins-close", IconKind::Close)
							.tooltip(cx.t("action-close"))
							.on_click(cx.listener(|_, _, _, cx| cx.emit(PinsEvent::Close))),
					),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.pt(theme.space_8)
					.children(cards)
					.when(loaded && count == 0, |list| {
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
									Icon::new(IconKind::Pin)
										.size(theme.portrait_lg)
										.color(theme.interactive_muted),
								)
								.child(cx.t("pins-empty")),
						)
					})
					.overflow_y_scrollbar()
					.id("pins-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
