use std::rc::Rc;

use gpui::{
	AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use store::Settings;

use super::widgets::{heading, row};
use crate::i18n::Translate as _;
use crate::shell::{SettingsBackend, set_reduced_motion};
use crate::theme::{Theme, rem};

const FONT_SCALE_MIN: f32 = 0.8;
const FONT_SCALE_MAX: f32 = 1.5;
const FONT_SCALE_STEP: f32 = 0.05;
const SATURATION_MAX: f32 = 100.;

pub struct AccessibilityPage {
	backend: Rc<SettingsBackend>,
	font_scale: Entity<SliderState>,
	saturation: Entity<SliderState>,
}

impl AccessibilityPage {
	pub fn new(backend: Rc<SettingsBackend>, cx: &mut Context<Self>) -> Self {
		let current = (backend.read)().accessibility;
		let font_scale = cx.new(|_| {
			SliderState::new()
				.min(FONT_SCALE_MIN)
				.max(FONT_SCALE_MAX)
				.step(FONT_SCALE_STEP)
				.default_value(current.font_scale.clamp(FONT_SCALE_MIN, FONT_SCALE_MAX))
		});
		let saturation = cx.new(|_| {
			SliderState::new()
				.min(0.)
				.max(SATURATION_MAX)
				.step(1.)
				.default_value(f32::from(current.saturation))
		});
		cx.subscribe(&font_scale, |this, _, event: &SliderEvent, cx| match event {
			SliderEvent::Change(value) => {
				Theme::rescale_fonts(value.end(), cx);
				cx.notify();
			}
			SliderEvent::Release(value) => {
				let scale = value.end();
				this.write(move |settings| settings.accessibility.font_scale = scale);
			}
		})
		.detach();
		cx.subscribe(&saturation, |this, _, event: &SliderEvent, cx| match event {
			SliderEvent::Change(_) => cx.notify(),
			SliderEvent::Release(value) => {
				let saturation = value.end().round() as u8;
				this.write(move |settings| settings.accessibility.saturation = saturation);
			}
		})
		.detach();
		Self {
			backend,
			font_scale,
			saturation,
		}
	}

	fn write(&self, apply: impl Fn(&mut Settings)) {
		(self.backend.write)(&apply);
	}

	fn set_reduced_motion(&mut self, reduced: bool, cx: &mut Context<Self>) {
		self.write(move |settings| settings.accessibility.reduced_motion = reduced);
		set_reduced_motion(reduced);
		cx.notify();
	}
}

fn percent_label(value: f32, theme: &Theme) -> impl IntoElement {
	div()
		.w(rem(48.))
		.text_size(theme.font_sm)
		.text_color(theme.text_muted)
		.child(format!("{}%", value.round()))
}

impl Render for AccessibilityPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let current = (self.backend.read)().accessibility;
		let font_scale = self.font_scale.read(cx).value().end();
		let saturation = self.saturation.read(cx).value().end();
		div()
			.flex()
			.flex_col()
			.child(heading(cx.t("settings-motion"), &theme))
			.child(row(
				cx.t("settings-reduced-motion"),
				None,
				Switch::new("accessibility-reduced-motion")
					.checked(current.reduced_motion)
					.on_change(
						cx.listener(|this, checked: &bool, _, cx| this.set_reduced_motion(*checked, cx)),
					),
				&theme,
			))
			.child(div().h(theme.space_16))
			.child(heading(cx.t("settings-font-scaling"), &theme))
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_16)
					.py(theme.space_12)
					.child(div().flex_1().child(Slider::new(&self.font_scale)))
					.child(percent_label(font_scale * 100., &theme)),
			)
			.child(heading(cx.t("accessibility-saturation"), &theme))
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_16)
					.py(theme.space_12)
					.child(div().flex_1().child(Slider::new(&self.saturation)))
					.child(percent_label(saturation, &theme)),
			)
	}
}
