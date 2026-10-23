use gpui::{
	AbsoluteLength, App, Hsla, IntoElement, RenderOnce, Styled as _, Window, div,
	prelude::FluentBuilder as _, px,
};

use crate::theme::Theme;

#[derive(IntoElement)]
pub struct Divider {
	vertical: bool,
	thickness: AbsoluteLength,
	length: Option<AbsoluteLength>,
	color: Option<Hsla>,
}

impl Divider {
	pub fn horizontal() -> Self {
		Self {
			vertical: false,
			thickness: px(1.).into(),
			length: None,
			color: None,
		}
	}

	pub fn vertical() -> Self {
		Self {
			vertical: true,
			..Self::horizontal()
		}
	}

	pub fn thickness(mut self, thickness: impl Into<AbsoluteLength>) -> Self {
		self.thickness = thickness.into();
		self
	}

	pub fn length(mut self, length: impl Into<AbsoluteLength>) -> Self {
		self.length = Some(length.into());
		self
	}

	pub fn color(mut self, color: Hsla) -> Self {
		self.color = Some(color);
		self
	}
}

impl RenderOnce for Divider {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let color = self.color.unwrap_or(cx.global::<Theme>().divider);
		let line = div().flex_none().bg(color);
		if self.vertical {
			line.w(self.thickness)
				.when_some(self.length, |line, length| line.h(length))
				.when(self.length.is_none(), |line| line.h_full())
		} else {
			line.h(self.thickness)
				.when_some(self.length, |line, length| line.w(length))
				.when(self.length.is_none(), |line| line.w_full())
		}
	}
}
