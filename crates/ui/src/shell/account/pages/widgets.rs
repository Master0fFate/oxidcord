use std::rc::Rc;

use gpui::{AnyElement, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, div};
use gpui_component::switch::Switch;
use store::Settings;

use crate::icons::{Icon, IconKind};
use crate::shell::SettingsBackend;
use crate::theme::Theme;

pub fn heading(text: SharedString, theme: &Theme) -> AnyElement {
	div()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(text.to_uppercase())
		.into_any_element()
}

pub fn row(
	title: SharedString,
	note: Option<SharedString>,
	control: impl IntoElement,
	theme: &Theme,
) -> AnyElement {
	div()
		.flex()
		.items_center()
		.justify_between()
		.gap(theme.space_16)
		.py(theme.space_12)
		.border_b_1()
		.border_color(theme.divider)
		.child(
			div()
				.flex()
				.flex_col()
				.flex_1()
				.min_w_0()
				.child(
					div()
						.text_size(theme.font_md)
						.font_weight(FontWeight::MEDIUM)
						.text_color(theme.header_primary)
						.child(title),
				)
				.children(note.map(|note| {
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_muted)
						.child(note)
				})),
		)
		.child(div().flex_none().child(control))
		.into_any_element()
}

pub fn toggle_row(
	id: &'static str,
	title: SharedString,
	note: Option<SharedString>,
	checked: bool,
	backend: &Rc<SettingsBackend>,
	apply: fn(&mut Settings, bool),
	theme: &Theme,
) -> AnyElement {
	let backend = backend.clone();
	let switch = Switch::new(id).checked(checked).on_change(move |checked, _, cx| {
		let checked = *checked;
		(backend.write)(&|settings| apply(settings, checked));
		cx.refresh_windows();
	});
	row(title, note, switch, theme)
}

pub fn section(title: SharedString, body: Vec<AnyElement>, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.child(heading(title, theme))
		.children(body)
		.into_any_element()
}

pub fn placeholder(icon: IconKind, note: SharedString, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_16)
		.p(theme.space_16)
		.rounded(theme.radius_md)
		.bg(theme.background_secondary)
		.child(Icon::new(icon).size(theme.icon_lg).color(theme.interactive_muted))
		.child(
			div()
				.text_size(theme.font_md)
				.text_color(theme.text_muted)
				.child(note),
		)
		.into_any_element()
}
