use gpui::{
	AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, MouseButton, ObjectFit,
	ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
	StyledImage as _, Window, WindowControlArea, div, img, prelude::FluentBuilder as _,
};
use media::SizeHint;

use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::images::Images;
use crate::theme::{Theme, rem};

const CONTROL_WIDTH: f32 = 46.;
const TITLE_ICON: f32 = 18.;
const TITLE_ICON_PIXELS: u32 = 36;
const MAC_TRAFFIC_LIGHTS: f32 = 80.;

pub struct TitleContext {
	pub name: SharedString,
	pub icon: Option<String>,
}

#[derive(IntoElement, Default)]
pub struct TitleBar {
	context: Option<TitleContext>,
	leading: Vec<AnyElement>,
	trailing: Vec<AnyElement>,
}

impl TitleBar {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn context(mut self, context: TitleContext) -> Self {
		self.context = Some(context);
		self
	}

	pub fn leading(mut self, items: impl IntoIterator<Item = AnyElement>) -> Self {
		self.leading.extend(items);
		self
	}

	pub fn trailing(mut self, items: impl IntoIterator<Item = AnyElement>) -> Self {
		self.trailing.extend(items);
		self
	}
}

fn title_icon(context: &TitleContext, theme: &Theme, cx: &App) -> AnyElement {
	let slot = div()
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size(rem(TITLE_ICON))
		.rounded(theme.radius_sm)
		.overflow_hidden();
	let image = context.icon.as_deref().and_then(|url| {
		cx.global::<Images>()
			.get(url, SizeHint::square(TITLE_ICON_PIXELS))
	});
	match image {
		Some(image) => slot
			.child(img(image).size_full().object_fit(ObjectFit::Cover))
			.into_any_element(),
		None => slot
			.bg(theme.background_secondary)
			.text_size(rem(9.))
			.font_weight(FontWeight::SEMIBOLD)
			.text_color(theme.header_secondary)
			.child(context.name.chars().next().map(String::from).unwrap_or_default())
			.into_any_element(),
	}
}

impl RenderOnce for TitleBar {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let maximized = window.is_maximized();
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
		let center = match &self.context {
			Some(context) => div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.min_w_0()
				.child(title_icon(context, theme, cx))
				.child(
					div()
						.truncate()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::MEDIUM)
						.text_color(theme.header_primary)
						.child(context.name.clone()),
				),
			None => div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(div().size(rem(10.)).rounded_full().bg(theme.brand))
				.child(
					div()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::MEDIUM)
						.text_color(theme.header_primary)
						.child(cx.t("app-name")),
				),
		};
		div()
			.relative()
			.flex()
			.flex_none()
			.items_center()
			.h(theme.title_bar_height)
			.w_full()
			.bg(theme.background_tertiary)
			.when(!self.leading.is_empty(), |bar| {
				bar.child(
					div()
						.flex()
						.flex_none()
						.items_center()
						.gap(theme.space_4)
						.pl(if cfg!(target_os = "macos") {
							rem(MAC_TRAFFIC_LIGHTS)
						} else {
							theme.space_12
						})
						.children(self.leading),
				)
			})
			.child(
				div()
					.id("title-drag")
					.flex_1()
					.h_full()
					.window_control_area(WindowControlArea::Drag)
					.when(cfg!(target_os = "linux"), |area| {
						area.on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
					}),
			)
			.child(
				div()
					.absolute()
					.inset_0()
					.flex()
					.items_center()
					.justify_center()
					.child(center),
			)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_8)
					.px(theme.space_8)
					.children(super::updates::UpdatesPage::title_prompt(theme, cx))
					.children(super::plugins::title_items(cx))
					.children(self.trailing),
			)
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
