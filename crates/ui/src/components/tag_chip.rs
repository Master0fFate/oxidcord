use gpui::{
	App, FontWeight, IntoElement, ObjectFit, ParentElement as _, RenderOnce, SharedString, Styled as _,
	StyledImage as _, Window, div, img, prelude::FluentBuilder as _,
};
use media::SizeHint;

use crate::images::Images;
use crate::theme::{Theme, rem, with_alpha};

const BADGE_SIZE: f32 = 12.;
const BADGE_PIXELS: u32 = 32;
const CHIP_HEIGHT: f32 = 16.;

#[derive(IntoElement)]
pub struct TagChip {
	text: SharedString,
	badge: Option<SharedString>,
}

impl TagChip {
	pub fn new(text: impl Into<SharedString>) -> Self {
		Self {
			text: text.into(),
			badge: None,
		}
	}

	pub fn badge(mut self, url: Option<SharedString>) -> Self {
		self.badge = url;
		self
	}
}

impl RenderOnce for TagChip {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let image = self
			.badge
			.as_ref()
			.and_then(|url| cx.global::<Images>().get(url, SizeHint::square(BADGE_PIXELS)));
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_4)
			.h(rem(CHIP_HEIGHT))
			.px(theme.space_4)
			.rounded(theme.radius_sm)
			.bg(with_alpha(theme.background_tertiary, 0.6))
			.text_size(theme.font_tiny)
			.line_height(rem(CHIP_HEIGHT))
			.font_weight(FontWeight::SEMIBOLD)
			.text_color(theme.text_muted)
			.when_some(image, |chip, image| {
				chip.child(
					div()
						.size(rem(BADGE_SIZE))
						.child(img(image).size_full().object_fit(ObjectFit::Contain)),
				)
			})
			.child(self.text)
	}
}
