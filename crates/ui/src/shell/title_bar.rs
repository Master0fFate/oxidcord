use gpui::{
	AbsoluteLength, App, FontWeight, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
	RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, WindowControlArea, div,
	prelude::FluentBuilder as _, px,
};

use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const CONTROL_WIDTH: f32 = 36.;

#[derive(IntoElement)]
pub struct TitleBar;

impl RenderOnce for TitleBar {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let title: SharedString = cx.t("app-name");
		let maximized = window.is_maximized();
		let leading: AbsoluteLength = if cfg!(target_os = "macos") {
			px(80.).into()
		} else {
			theme.space_12.into()
		};
		let controls = [
			(
				"title-minimize",
				IconKind::WindowMinimize,
				WindowControlArea::Min,
				false,
			),
			(
				"title-maximize",
				if maximized {
					IconKind::WindowRestore
				} else {
					IconKind::WindowMaximize
				},
				WindowControlArea::Max,
				false,
			),
			(
				"title-close",
				IconKind::WindowClose,
				WindowControlArea::Close,
				true,
			),
		];
		let rest = theme.interactive_normal;
		let hover_background = theme.modifier_selected;
		let close_background = theme.danger;
		let close_foreground = theme.text_on_brand;
		div()
			.flex()
			.flex_none()
			.items_center()
			.h(theme.title_bar_height)
			.w_full()
			.bg(theme.background_tertiary)
			.child(
				div()
					.id("title-drag")
					.flex()
					.flex_1()
					.items_center()
					.h_full()
					.gap(theme.space_8)
					.pl(leading)
					.window_control_area(WindowControlArea::Drag)
					.when(cfg!(target_os = "linux"), |area| {
						area.on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
					})
					.child(div().size(rem(10.)).rounded_full().bg(theme.brand))
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.text_muted)
							.child(title),
					),
			)
			.children(super::plugins::title_items(cx))
			.when(!cfg!(target_os = "macos"), |bar| {
				bar.children(controls.into_iter().map(|(id, icon, area, closes)| {
					div()
						.id(id)
						.flex()
						.items_center()
						.justify_center()
						.w(rem(CONTROL_WIDTH))
						.h_full()
						.window_control_area(area)
						.hover(move |style| {
							if closes {
								style.bg(close_background).text_color(close_foreground)
							} else {
								style.bg(hover_background)
							}
						})
						.when(cfg!(target_os = "linux"), |button| {
							button.on_click(move |_, window, _| match area {
								WindowControlArea::Min => window.minimize_window(),
								WindowControlArea::Max => window.zoom_window(),
								_ => window.remove_window(),
							})
						})
						.child(Icon::new(icon).size(rem(10.)).color(rest))
				}))
			})
	}
}
