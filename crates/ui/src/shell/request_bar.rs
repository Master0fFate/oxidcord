use gpui::{
	AnyElement, App, ClickEvent, Entity, FontWeight, IntoElement, ParentElement as _, Styled as _, Window,
	div,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use session::Command;
use state::RequestRow;

use crate::data::Data;
use crate::i18n::Translate as _;
use crate::theme::Theme;

pub fn request_bar(data: &Entity<Data>, request: &RequestRow, theme: &Theme, cx: &App) -> AnyElement {
	let channel_id = request.channel_id;
	let user_id = request.person.user_id;
	let accept = data.clone();
	let ignore = data.clone();
	let block = data.clone();
	let title = if request.spam {
		"request-bar-spam"
	} else {
		"request-bar-title"
	};
	div()
		.flex()
		.flex_none()
		.items_center()
		.justify_between()
		.gap(theme.space_12)
		.px(theme.space_16)
		.py(theme.space_8)
		.border_t_1()
		.border_color(theme.divider)
		.bg(theme.background_secondary)
		.child(
			div()
				.truncate()
				.text_size(theme.font_md)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(theme.header_primary)
				.child(cx.t(title)),
		)
		.child(
			div()
				.flex()
				.flex_none()
				.items_center()
				.gap(theme.space_8)
				.child(
					Button::new("request-accept")
						.small()
						.primary()
						.label(cx.t("friends-accept"))
						.on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
							accept.read(cx).send(Command::AcceptMessageRequest { channel_id });
						}),
				)
				.child(
					Button::new("request-ignore")
						.small()
						.outline()
						.label(cx.t("friends-ignore"))
						.on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
							ignore.read(cx).send(Command::RejectMessageRequests {
								channel_ids: vec![channel_id],
							});
						}),
				)
				.child(
					Button::new("request-block")
						.small()
						.danger()
						.label(cx.t("friends-block"))
						.on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
							let data = block.read(cx);
							data.send(Command::BlockUser { user_id });
							data.send(Command::RejectMessageRequests {
								channel_ids: vec![channel_id],
							});
						}),
				),
		)
		.into_any_element()
}
