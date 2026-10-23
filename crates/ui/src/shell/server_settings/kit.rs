use gpui::{
	AnyElement, App, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};

use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

pub fn title(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
	div()
		.flex_none()
		.pb(theme.space_20)
		.text_size(theme.font_xl)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_primary)
		.child(text.into())
		.into_any_element()
}

pub fn empty(icon: IconKind, text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.items_center()
		.gap(theme.space_8)
		.p(theme.space_32)
		.text_size(theme.font_md)
		.text_color(theme.text_muted)
		.child(
			Icon::new(icon)
				.size(theme.portrait_lg)
				.color(theme.interactive_muted),
		)
		.child(text.into())
		.into_any_element()
}

pub fn save_bar(
	theme: &Theme,
	cx: &App,
	on_reset: impl Fn(&mut Window, &mut App) + 'static,
	on_save: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
	div()
		.flex()
		.flex_none()
		.items_center()
		.justify_between()
		.gap(theme.space_16)
		.mb(theme.space_16)
		.px(theme.space_16)
		.py(theme.space_12)
		.rounded(theme.radius_md)
		.bg(theme.background_floating)
		.border_1()
		.border_color(theme.divider)
		.child(
			div()
				.text_size(theme.font_md)
				.text_color(theme.text_normal)
				.child(cx.t("settings-unsaved")),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(
					Button::new("settings-reset")
						.ghost()
						.label(cx.t("settings-reset"))
						.on_click(move |_, window, cx| on_reset(window, cx)),
				)
				.child(
					Button::new("settings-save")
						.success()
						.label(cx.t("settings-save"))
						.on_click(move |_, window, cx| on_save(window, cx)),
				),
		)
		.into_any_element()
}

pub fn count_caption(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
	div()
		.pb(theme.space_8)
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(text.into().to_uppercase())
		.into_any_element()
}
