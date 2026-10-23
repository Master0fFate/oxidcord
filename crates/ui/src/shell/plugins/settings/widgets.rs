use gpui::{
	AnyElement, App, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
	SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};

use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem, with_alpha};

const CHIP_HEIGHT: f32 = 20.;
const CHIP_FILL: f32 = 0.18;
const ICON_TILE: f32 = 44.;
const ICON_GLYPH: f32 = 24.;
const BANNER_FILL: f32 = 0.14;

pub fn section_label(key: &str, theme: &Theme, cx: &App) -> AnyElement {
	div()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(cx.t(key).to_uppercase())
		.into_any_element()
}

pub fn chip(text: impl Into<SharedString>, color: Hsla, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_none()
		.items_center()
		.h(rem(CHIP_HEIGHT))
		.px(theme.space_8)
		.rounded(theme.radius_sm)
		.bg(with_alpha(color, CHIP_FILL))
		.text_color(color)
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.child(text.into().to_uppercase())
		.into_any_element()
}

pub fn card(theme: &Theme) -> gpui::Div {
	div()
		.flex()
		.flex_col()
		.gap(theme.space_12)
		.p(theme.space_16)
		.rounded(theme.radius_md)
		.bg(theme.background_secondary)
}

pub fn icon_tile(icon: IconKind, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size(rem(ICON_TILE))
		.rounded(theme.radius_md)
		.bg(with_alpha(theme.brand, CHIP_FILL))
		.child(Icon::new(icon).size(rem(ICON_GLYPH)).color(theme.brand))
		.into_any_element()
}

pub fn banner(text: impl Into<SharedString>, color: Hsla, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_8)
		.px(theme.space_12)
		.py(theme.space_8)
		.rounded(theme.radius_sm)
		.border_l_4()
		.border_color(color)
		.bg(with_alpha(color, BANNER_FILL))
		.text_size(theme.font_sm)
		.text_color(theme.text_normal)
		.child(text.into())
		.into_any_element()
}

pub fn heading(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
	div()
		.text_size(theme.font_md)
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(theme.header_primary)
		.child(text.into())
		.into_any_element()
}

pub fn note(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
	div()
		.text_size(theme.font_sm)
		.text_color(theme.text_muted)
		.child(text.into())
		.into_any_element()
}

pub fn tab_pill(
	id: &'static str,
	label: SharedString,
	active: bool,
	theme: &Theme,
	on_click: impl Fn(&mut gpui::Window, &mut App) + 'static,
) -> AnyElement {
	let hover = theme.modifier_hover;
	div()
		.id(id)
		.flex()
		.items_center()
		.h(rem(32.))
		.px(theme.space_12)
		.rounded(theme.radius_sm)
		.cursor_pointer()
		.text_size(theme.font_sm)
		.font_weight(FontWeight::MEDIUM)
		.text_color(if active {
			theme.interactive_active
		} else {
			theme.interactive_normal
		})
		.when(active, |pill| pill.bg(theme.modifier_selected))
		.when(!active, |pill| pill.hover(move |style| style.bg(hover)))
		.on_click(move |_, window, cx| on_click(window, cx))
		.child(label)
		.into_any_element()
}
