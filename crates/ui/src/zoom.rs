use std::sync::Arc;

use gpui::{App, Global, Window};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use store::Store;

use crate::actions::{ZoomIn, ZoomOut, ZoomReset};
use crate::i18n::Translate as _;
use crate::images;
use crate::theme::{Theme, rem_pixels};

pub const MIN: f32 = 0.5;
pub const MAX: f32 = 2.0;
pub const STEP: f32 = 0.1;

struct Persist(Arc<Store>);

struct ZoomToast;

impl Global for Persist {}

pub fn install(store: Arc<Store>, cx: &mut App) {
	let scale = clamp(store.settings.read().appearance.ui_scale);
	cx.set_global(Persist(store));
	images::set_ui_scale(scale);
	Theme::set_ui_scale(scale, cx);
	cx.on_action(|_: &ZoomIn, cx| shortcut(current(cx) + STEP, cx));
	cx.on_action(|_: &ZoomOut, cx| shortcut(current(cx) - STEP, cx));
	cx.on_action(|_: &ZoomReset, cx| shortcut(1., cx));
}

pub fn attach(window: &mut Window, cx: &App) {
	window.set_rem_size(rem_pixels(current(cx)));
}

pub fn current(cx: &App) -> f32 {
	cx.global::<Theme>().ui_scale
}

pub fn apply(scale: f32, cx: &mut App) {
	let scale = clamp(scale);
	images::set_ui_scale(scale);
	Theme::set_ui_scale(scale, cx);
	cx.defer(move |cx| {
		for handle in cx.windows() {
			handle
				.update(cx, |_, window, _| {
					window.set_rem_size(rem_pixels(scale));
					window.refresh();
				})
				.ok();
		}
	});
}

pub fn persist(scale: f32, cx: &App) {
	let scale = clamp(scale);
	if let Err(error) = cx
		.global::<Persist>()
		.0
		.update_settings(|settings| settings.appearance.ui_scale = scale)
	{
		tracing::warn!(%error, "could not save the ui scale");
	}
}

pub fn set(scale: f32, cx: &mut App) {
	apply(scale, cx);
	persist(scale, cx);
}

fn shortcut(target: f32, cx: &mut App) {
	let scale = clamp((target / STEP).round() * STEP);
	set(scale, cx);
	let message = format!("{}: {}%", cx.t("settings-dpi-scale"), (scale * 100.).round());
	cx.defer(move |cx| {
		if let Some(handle) = cx.active_window() {
			handle
				.update(cx, |_, window, cx| {
					window.push_notification(Notification::info(message).id::<ZoomToast>(), cx);
				})
				.ok();
		}
	});
}

fn clamp(scale: f32) -> f32 {
	if scale.is_finite() {
		scale.clamp(MIN, MAX)
	} else {
		1.
	}
}
