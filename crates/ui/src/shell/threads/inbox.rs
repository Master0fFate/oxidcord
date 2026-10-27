use gpui::{
	AnyElement, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render,
	Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Message, Snowflake};
use session::Command;
use std::sync::Arc;

use crate::components::{Avatar, IconButton};
use crate::data::Data;
use crate::format;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::lookup::user_avatar;
use crate::shell::message::snippet;
use crate::theme::Theme;

const SNIPPET_LENGTH: usize = 260;

pub enum InboxEvent {
	Jump { channel: Snowflake, message: Snowflake },
	Close,
}

pub struct InboxPanel {
	data: Entity<Data>,
}

impl EventEmitter<InboxEvent> for InboxPanel {}

impl InboxPanel {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadMentions);
		Self { data }
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		cx.notify();
	}

	fn card(&self, message: &Arc<Message>, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = message.id;
		let channel = message.channel_id;
		let (name, place) = {
			let data = self.data.read(cx);
			let state = data.state();
			let guild = state.channel_guild(channel);
			let place = match (state.channel(channel).and_then(|found| found.name.clone()), guild) {
				(Some(channel_name), Some(guild_id)) => match state.guild(guild_id) {
					Some(found) => format!("{} \u{b7} #{channel_name}", found.name),
					None => format!("#{channel_name}"),
				},
				(Some(channel_name), None) => channel_name,
				(None, _) => String::new(),
			};
			(state.display_name(guild, message.author.id), place)
		};
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
					.truncate()
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.text_muted)
					.child(place),
			)
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
						Button::new(("inbox-jump", id.get()))
							.small()
							.outline()
							.label(cx.t("pin-jump"))
							.on_click(cx.listener(move |_, _, _, cx| {
								cx.emit(InboxEvent::Jump { channel, message: id })
							})),
					),
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

impl Render for InboxPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let messages: Option<Vec<Arc<Message>>> = self
			.data
			.read(cx)
			.state()
			.mentions()
			.map(|mentions| mentions.to_vec());
		let cards: Vec<AnyElement> = messages
			.iter()
			.flatten()
			.map(|message| self.card(message, &theme, cx))
			.collect();
		let status = match &messages {
			None => Some(("inbox-loading", IconKind::Inbox)),
			Some(found) if found.is_empty() => Some(("inbox-empty", IconKind::Inbox)),
			Some(_) => None,
		};
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
						Icon::new(IconKind::Inbox)
							.size(theme.glyph_lg)
							.color(theme.text_muted),
					)
					.child(
						div()
							.flex_1()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(cx.t("inbox-title")),
					)
					.child(
						IconButton::new("inbox-close", IconKind::Close)
							.tooltip(cx.t("action-close"))
							.on_click(cx.listener(|_, _, _, cx| cx.emit(InboxEvent::Close))),
					),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.pt(theme.space_8)
					.children(cards)
					.when_some(status, |list, (label, icon)| {
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
									Icon::new(icon)
										.size(theme.portrait_lg)
										.color(theme.interactive_muted),
								)
								.child(cx.t(label)),
						)
					})
					.overflow_y_scrollbar()
					.id("inbox-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
