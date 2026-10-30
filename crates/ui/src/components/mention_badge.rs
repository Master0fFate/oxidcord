use gpui::{App, FontWeight, Hsla, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, div};

use crate::theme::{Theme, rem};

const MAX_DISPLAYED: u32 = 999;
const BADGE_HEIGHT: f32 = 16.;
const BADGE_PADDING: f32 = 5.;
const BADGE_TEXT: f32 = 12.;
const RING_WIDTH: f32 = 3.;

#[derive(IntoElement)]
pub struct MentionBadge {
	count: u32,
	ring: Option<Hsla>,
}

impl MentionBadge {
	pub fn new(count: u32) -> Self {
		Self { count, ring: None }
	}

	pub fn ring(mut self, color: Hsla) -> Self {
		self.ring = Some(color);
		self
	}
}

impl RenderOnce for MentionBadge {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let label = if self.count > MAX_DISPLAYED {
			format!("{MAX_DISPLAYED}+")
		} else {
			self.count.to_string()
		};
		let badge = div()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.min_w(rem(BADGE_HEIGHT))
			.h(rem(BADGE_HEIGHT))
			.px(rem(BADGE_PADDING))
			.rounded(theme.round(rem(BADGE_HEIGHT)))
			.bg(theme.mention_badge)
			.text_color(theme.text_on_brand)
			.text_size(rem(BADGE_TEXT))
			.font_weight(FontWeight::BOLD)
			.line_height(rem(BADGE_HEIGHT))
			.text_center()
			.whitespace_nowrap()
			.child(label);
		match self.ring {
			Some(ring) => div()
				.flex()
				.flex_none()
				.p(rem(RING_WIDTH))
				.rounded(theme.round(rem(BADGE_HEIGHT + RING_WIDTH * 2.)))
				.bg(ring)
				.child(badge),
			None => badge,
		}
	}
}
