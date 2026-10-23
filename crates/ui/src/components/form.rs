use std::rc::Rc;

use gpui::{
	AnyElement, App, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
	SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Disableable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::switch::Switch;

use super::context_menu::ContextMenuBuilder;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem, with_alpha};

pub type PickHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;
pub type TriHandler = Rc<dyn Fn(Tri, &mut Window, &mut App)>;
pub type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

pub fn caption(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
	div()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(text.into().to_uppercase())
		.into_any_element()
}

pub fn field(label: impl Into<SharedString>, theme: &Theme, content: impl IntoElement) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.child(caption(label, theme))
		.child(content)
		.into_any_element()
}

pub fn hint(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
	div()
		.text_size(theme.font_sm)
		.text_color(theme.text_muted)
		.child(text.into())
		.into_any_element()
}

pub fn switch_row(
	id: impl Into<ElementId>,
	title: impl Into<SharedString>,
	description: Option<SharedString>,
	checked: bool,
	theme: &Theme,
	on_change: ToggleHandler,
) -> AnyElement {
	div()
		.flex()
		.items_center()
		.justify_between()
		.gap(theme.space_16)
		.child(
			div()
				.flex()
				.flex_col()
				.min_w_0()
				.child(
					div()
						.text_size(theme.font_md)
						.font_weight(FontWeight::MEDIUM)
						.text_color(theme.header_primary)
						.child(title.into()),
				)
				.children(description.map(|text| hint(text, theme))),
		)
		.child(
			Switch::new(id)
				.checked(checked)
				.on_click(move |value: &bool, window, cx| on_change(*value, window, cx)),
		)
		.into_any_element()
}

pub fn select(
	id: impl Into<ElementId>,
	current: impl Into<SharedString>,
	options: &[SharedString],
	selected: usize,
	on_pick: PickHandler,
) -> impl IntoElement {
	let menu = options
		.iter()
		.enumerate()
		.fold(ContextMenuBuilder::new(), |menu, (index, label)| {
			let on_pick = on_pick.clone();
			menu.checked_item(label.clone(), index == selected, move |window, cx| {
				on_pick(index, window, cx)
			})
		})
		.build();
	Button::new(id)
		.outline()
		.label(current)
		.dropdown_caret(true)
		.dropdown_menu(menu)
}

pub fn segmented(
	id: &'static str,
	options: &[SharedString],
	selected: usize,
	theme: &Theme,
	on_pick: PickHandler,
) -> AnyElement {
	let hover = theme.modifier_hover;
	div()
		.flex()
		.flex_wrap()
		.gap(theme.space_8)
		.children(options.iter().enumerate().map(|(index, label)| {
			let on_pick = on_pick.clone();
			let active = index == selected;
			div()
				.id((id, index))
				.flex()
				.items_center()
				.h(rem(32.))
				.px(theme.space_12)
				.rounded(theme.radius_sm)
				.cursor_pointer()
				.border_1()
				.border_color(if active { theme.brand } else { theme.divider })
				.bg(if active {
					with_alpha(theme.brand, 0.16)
				} else {
					theme.background_secondary
				})
				.text_size(theme.font_sm)
				.font_weight(FontWeight::MEDIUM)
				.text_color(if active {
					theme.interactive_active
				} else {
					theme.interactive_normal
				})
				.when(!active, |pill| pill.hover(move |style| style.bg(hover)))
				.on_click(move |_, window, cx| on_pick(index, window, cx))
				.child(label.clone())
		}))
		.into_any_element()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tri {
	Deny,
	Neutral,
	Allow,
}

pub fn tri_state(id: impl Into<ElementId>, value: Tri, theme: &Theme, on_change: TriHandler) -> AnyElement {
	let id: ElementId = id.into();
	let key: SharedString = id.to_string().into();
	let cell = |tri: Tri, icon: IconKind, color: Hsla| {
		let active = value == tri;
		let on_change = on_change.clone();
		div()
			.id(ElementId::NamedInteger(key.clone(), tri as u64))
			.flex()
			.items_center()
			.justify_center()
			.size(rem(28.))
			.cursor_pointer()
			.bg(if active {
				with_alpha(color, 0.9)
			} else {
				theme.background_tertiary
			})
			.on_click(move |_, window, cx| on_change(tri, window, cx))
			.child(Icon::new(icon).size(rem(16.)).color(if active {
				theme.text_on_brand
			} else {
				theme.interactive_muted
			}))
	};
	div()
		.flex()
		.flex_none()
		.rounded(theme.radius_sm)
		.overflow_hidden()
		.border_1()
		.border_color(theme.divider)
		.child(cell(Tri::Deny, IconKind::Close, theme.danger))
		.child(cell(Tri::Neutral, IconKind::Minus, theme.interactive_muted))
		.child(cell(Tri::Allow, IconKind::Check, theme.success))
		.into_any_element()
}

pub fn dialog_footer(
	cancel: impl Into<SharedString>,
	confirm: impl Into<SharedString>,
	danger: bool,
	enabled: bool,
	theme: &Theme,
	on_cancel: impl Fn(&mut Window, &mut App) + 'static,
	on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
	let confirm_button = Button::new("dialog-confirm")
		.label(confirm)
		.disabled(!enabled)
		.on_click(move |_, window, cx| on_confirm(window, cx));
	let confirm_button = if danger {
		confirm_button.danger()
	} else {
		confirm_button.primary()
	};
	div()
		.flex()
		.items_center()
		.justify_end()
		.gap(theme.space_8)
		.child(
			Button::new("dialog-cancel")
				.ghost()
				.label(cancel)
				.on_click(move |_, window, cx| on_cancel(window, cx)),
		)
		.child(confirm_button)
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

pub fn badge(text: impl Into<SharedString>, color: Hsla, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_none()
		.items_center()
		.h(rem(20.))
		.px(theme.space_8)
		.rounded(theme.radius_sm)
		.bg(with_alpha(color, 0.18))
		.text_color(color)
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.child(text.into())
		.into_any_element()
}
