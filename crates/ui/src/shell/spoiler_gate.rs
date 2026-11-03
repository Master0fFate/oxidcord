use gpui::{AnyElement, App, FontWeight, IntoElement, ParentElement as _, Styled as _, Window, div};
use gpui_component::button::{Button, ButtonVariants as _};

use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

pub fn spoiler_gate(
	theme: &Theme,
	cx: &App,
	on_continue: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.flex_1()
		.min_h_0()
		.items_center()
		.justify_center()
		.gap(theme.space_12)
		.p(theme.space_32)
		.child(
			Icon::new(IconKind::Shield)
				.size(theme.portrait_lg)
				.color(theme.interactive_muted),
		)
		.child(
			div()
				.text_size(theme.font_xl)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_primary)
				.child(cx.t("spoiler-gate-title")),
		)
		.child(
			div()
				.text_size(theme.font_md)
				.text_color(theme.text_muted)
				.child(cx.t("spoiler-gate-body")),
		)
		.child(
			Button::new("spoiler-gate-continue")
				.primary()
				.label(cx.t("spoiler-gate-continue"))
				.on_click(move |_, window, cx| on_continue(window, cx)),
		)
		.into_any_element()
}
