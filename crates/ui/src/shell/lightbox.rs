use std::rc::Rc;

use gpui::{
	AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, MouseButton, ObjectFit,
	ParentElement as _, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div, img, px,
};
use media::SizeHint;

use super::SettingsBackend;
use super::links;
use super::message::ImageTarget;
use super::plugins::LinkOrigin;
use crate::components::IconButton;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::images::Images;
use crate::theme::{Theme, rem};

pub type CloseFn = Rc<dyn Fn(&mut Window, &mut App)>;

const VIEWPORT_SHARE: f32 = 0.88;
const FALLBACK_SIDE: f32 = 600.;

pub fn lightbox(
	target: &ImageTarget,
	backend: &Rc<SettingsBackend>,
	window: &Window,
	theme: &Theme,
	cx: &App,
	close: CloseFn,
) -> AnyElement {
	let viewport = window.viewport_size();
	let max_width = f32::from(viewport.width) * VIEWPORT_SHARE;
	let max_height = f32::from(viewport.height) * VIEWPORT_SHARE;
	let (native_width, native_height) = if target.width > 0 && target.height > 0 {
		(target.width as f32, target.height as f32)
	} else {
		(FALLBACK_SIDE, FALLBACK_SIDE)
	};
	let scale = (max_width / native_width).min(max_height / native_height);
	let (width, height) = ((native_width * scale).round(), (native_height * scale).round());
	let image = cx.global::<Images>().get(&target.url, SizeHint::ORIGINAL);
	let url = target.url.clone();
	let backend = backend.clone();
	let backdrop_close = close.clone();
	let button_close = close;
	let shown = match image {
		Some(image) => img(image)
			.size_full()
			.object_fit(ObjectFit::Contain)
			.into_any_element(),
		None => div()
			.size_full()
			.flex()
			.items_center()
			.justify_center()
			.text_color(theme.text_muted)
			.child(cx.t("lightbox-loading"))
			.into_any_element(),
	};
	div()
		.id("lightbox")
		.absolute()
		.inset_0()
		.flex()
		.flex_col()
		.items_center()
		.justify_center()
		.gap(theme.space_12)
		.bg(theme.overlay)
		.occlude()
		.on_click(move |_, window, cx| backdrop_close(window, cx))
		.child(
			div()
				.absolute()
				.top(theme.space_16)
				.right(theme.space_16)
				.flex()
				.gap(theme.space_8)
				.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
				.child(
					IconButton::new("lightbox-open", IconKind::ExternalLink)
						.filled_hover()
						.hit_size(rem(36.))
						.icon_size(rem(22.))
						.color(theme.interactive_active)
						.tooltip(cx.t("lightbox-open"))
						.on_click(move |_, window, cx| {
							links::open_link(&url, LinkOrigin::default(), &backend, window, cx)
						}),
				)
				.child(
					IconButton::new("lightbox-close", IconKind::Close)
						.filled_hover()
						.hit_size(rem(36.))
						.icon_size(rem(22.))
						.color(theme.interactive_active)
						.tooltip(cx.t("lightbox-close"))
						.on_click(move |_, window, cx| button_close(window, cx)),
				),
		)
		.child(
			div()
				.w(px(width))
				.h(px(height))
				.rounded(theme.radius_md)
				.overflow_hidden()
				.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
				.child(shown),
		)
		.child(
			div()
				.text_size(theme.font_sm)
				.font_weight(FontWeight::MEDIUM)
				.text_color(theme.interactive_active)
				.child(target.name.clone()),
		)
		.into_any_element()
}
