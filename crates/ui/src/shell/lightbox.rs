use std::rc::Rc;

use gpui::{
	App, Context, FocusHandle, Focusable, FontWeight, InteractiveElement as _, IntoElement, MouseButton,
	MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement as _, Render, ScrollWheelEvent,
	StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window, div, img,
	prelude::FluentBuilder as _, px,
};
use media::SizeHint;

use super::SettingsBackend;
use super::community::save_file;
use super::links;
use super::message::{Gallery, ImageTarget};
use super::plugins::LinkOrigin;
use crate::actions::{LIGHTBOX_CONTEXT, LightboxNext, LightboxPrevious};
use crate::components::IconButton;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::images::Images;
use crate::theme::{Theme, rem};

pub type CloseFn = Rc<dyn Fn(&mut Window, &mut App)>;

const VIEWPORT_SHARE: f32 = 0.88;
const FALLBACK_SIDE: f32 = 600.;
const ZOOM_CLICK: f32 = 2.5;
const ZOOM_MAX: f32 = 8.;
const ZOOM_PER_PIXEL: f32 = 0.0015;
const WHEEL_LINE: f32 = 16.;
const DRAG_SLOP: f32 = 4.;

struct Drag {
	from: (f32, f32),
	pan: (f32, f32),
	moved: bool,
}

pub struct Lightbox {
	items: Rc<[ImageTarget]>,
	index: usize,
	zoom: f32,
	pan: (f32, f32),
	drag: Option<Drag>,
	focus: FocusHandle,
	backend: Rc<SettingsBackend>,
	close: CloseFn,
}

impl Focusable for Lightbox {
	fn focus_handle(&self, _: &App) -> FocusHandle {
		self.focus.clone()
	}
}

impl Lightbox {
	pub fn new(
		gallery: Gallery,
		backend: Rc<SettingsBackend>,
		close: CloseFn,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let focus = cx.focus_handle();
		window.focus(&focus, cx);
		Self {
			index: gallery.start.min(gallery.items.len().saturating_sub(1)),
			items: gallery.items,
			zoom: 1.,
			pan: (0., 0.),
			drag: None,
			focus,
			backend,
			close,
		}
	}

	fn fit(&self, window: &Window) -> (f32, f32) {
		let viewport = window.viewport_size();
		let item = &self.items[self.index];
		let (width, height) = if item.width > 0 && item.height > 0 {
			(item.width as f32, item.height as f32)
		} else {
			(FALLBACK_SIDE, FALLBACK_SIDE)
		};
		let scale = (f32::from(viewport.width) * VIEWPORT_SHARE / width)
			.min(f32::from(viewport.height) * VIEWPORT_SHARE / height);
		((width * scale).round(), (height * scale).round())
	}

	fn limit_pan(&mut self, window: &Window) {
		let (width, height) = self.fit(window);
		let reach = (
			(width * (self.zoom - 1.) / 2.).max(0.),
			(height * (self.zoom - 1.) / 2.).max(0.),
		);
		self.pan = (
			self.pan.0.clamp(-reach.0, reach.0),
			self.pan.1.clamp(-reach.1, reach.1),
		);
	}

	fn step(&mut self, direction: isize, cx: &mut Context<Self>) {
		let index = self.index.saturating_add_signed(direction);
		if index >= self.items.len() || index == self.index {
			return;
		}
		self.index = index;
		self.zoom = 1.;
		self.pan = (0., 0.);
		self.drag = None;
		cx.notify();
	}

	fn zoom_to(&mut self, zoom: f32, window: &Window, cx: &mut Context<Self>) {
		self.zoom = zoom.clamp(1., ZOOM_MAX);
		if self.zoom == 1. {
			self.pan = (0., 0.);
		}
		self.limit_pan(window);
		cx.notify();
	}

	fn on_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
		let lines = px(WHEEL_LINE);
		let delta = f32::from(event.delta.pixel_delta(lines).y);
		self.zoom_to(self.zoom * (1. + delta * ZOOM_PER_PIXEL), window, cx);
	}

	fn on_press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
		cx.stop_propagation();
		self.drag = Some(Drag {
			from: (f32::from(event.position.x), f32::from(event.position.y)),
			pan: self.pan,
			moved: false,
		});
	}

	fn on_drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
		let Some(drag) = &mut self.drag else {
			return;
		};
		let delta = (
			f32::from(event.position.x) - drag.from.0,
			f32::from(event.position.y) - drag.from.1,
		);
		drag.moved |= delta.0.hypot(delta.1) > DRAG_SLOP;
		if !drag.moved || self.zoom == 1. {
			return;
		}
		self.pan = (drag.pan.0 + delta.0, drag.pan.1 + delta.1);
		self.limit_pan(window);
		cx.notify();
	}

	fn on_release(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
		let Some(drag) = self.drag.take() else {
			return;
		};
		if drag.moved {
			return;
		}
		let zoom = if self.zoom == 1. { ZOOM_CLICK } else { 1. };
		self.zoom_to(zoom, window, cx);
	}
}

impl Render for Lightbox {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let item = &self.items[self.index];
		let (width, height) = self.fit(window);
		let shown = match cx.global::<Images>().get(&item.fetch, SizeHint::ORIGINAL) {
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
		let several = self.items.len() > 1;
		let close = self.close.clone();
		let button_close = self.close.clone();
		let toolbar_button = |id: &'static str, icon: IconKind| {
			IconButton::new(id, icon)
				.filled_hover()
				.hit_size(rem(36.))
				.icon_size(rem(22.))
				.color(theme.interactive_active)
		};
		let arrow = |id: &'static str, icon: IconKind, direction: isize| {
			toolbar_button(id, icon).on_click(cx.listener(move |this, _, _, cx| this.step(direction, cx)))
		};
		div()
			.id("lightbox")
			.key_context(LIGHTBOX_CONTEXT)
			.track_focus(&self.focus)
			.on_action(cx.listener(|this, _: &LightboxPrevious, _, cx| this.step(-1, cx)))
			.on_action(cx.listener(|this, _: &LightboxNext, _, cx| this.step(1, cx)))
			.relative()
			.size_full()
			.flex()
			.flex_col()
			.items_center()
			.justify_center()
			.gap(theme.space_12)
			.bg(theme.overlay)
			.occlude()
			.on_scroll_wheel(cx.listener(Self::on_wheel))
			.on_click(move |_, window, cx| close(window, cx))
			.child(
				div()
					.absolute()
					.top(theme.space_16)
					.right(theme.space_16)
					.flex()
					.items_center()
					.gap(theme.space_8)
					.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
					.when(several, |bar| {
						bar.child(
							div()
								.px(theme.space_8)
								.text_size(theme.font_sm)
								.font_weight(FontWeight::MEDIUM)
								.text_color(theme.interactive_active)
								.child(cx.t_args(
									"lightbox-count",
									&[
										("current", ArgValue::Number((self.index + 1) as f64)),
										("total", ArgValue::Number(self.items.len() as f64)),
									],
								)),
						)
					})
					.child(
						toolbar_button("lightbox-copy", IconKind::Copy)
							.tooltip(cx.t("menu-copy-image"))
							.on_click(cx.listener(|this, _, _, cx| {
								cx.global::<Images>().copy_image(&this.items[this.index].fetch)
							})),
					)
					.child(
						toolbar_button("lightbox-save", IconKind::Download)
							.tooltip(cx.t("menu-save-image"))
							.on_click(cx.listener(|this, _, _, cx| {
								let item = &this.items[this.index];
								save_file(&item.fetch, item.name.clone(), cx)
							})),
					)
					.child(
						toolbar_button("lightbox-open", IconKind::ExternalLink)
							.tooltip(cx.t("lightbox-open"))
							.on_click(cx.listener(|this, _, window, cx| {
								links::open_link(
									&this.items[this.index].url,
									LinkOrigin::default(),
									&this.backend,
									window,
									cx,
								)
							})),
					)
					.child(
						toolbar_button("lightbox-close", IconKind::Close)
							.tooltip(cx.t("lightbox-close"))
							.on_click(move |_, window, cx| button_close(window, cx)),
					),
			)
			.when(several && self.index > 0, |root| {
				root.child(
					div()
						.absolute()
						.left(theme.space_16)
						.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
						.child(arrow("lightbox-previous", IconKind::ChevronLeft, -1)),
				)
			})
			.when(several && self.index + 1 < self.items.len(), |root| {
				root.child(
					div()
						.absolute()
						.right(theme.space_16)
						.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
						.child(arrow("lightbox-next", IconKind::ChevronRight, 1)),
				)
			})
			.child(
				div()
					.relative()
					.w(px(width))
					.h(px(height))
					.rounded(theme.radius_md)
					.overflow_hidden()
					.cursor_pointer()
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(|this, event: &MouseDownEvent, _, cx| this.on_press(event, cx)),
					)
					.on_mouse_move(cx.listener(Self::on_drag))
					.on_mouse_up(MouseButton::Left, cx.listener(Self::on_release))
					.on_mouse_up_out(
						MouseButton::Left,
						cx.listener(|this, _, _, _| {
							this.drag = None;
						}),
					)
					.child(
						div()
							.absolute()
							.left(px(self.pan.0 - width * (self.zoom - 1.) / 2.))
							.top(px(self.pan.1 - height * (self.zoom - 1.) / 2.))
							.w(px(width * self.zoom))
							.h(px(height * self.zoom))
							.child(shown),
					),
			)
			.child(
				div()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::MEDIUM)
					.text_color(theme.interactive_active)
					.child(item.name.clone()),
			)
	}
}
