use gpui::{App, FontWeight, Hsla, IntoElement, ParentElement as _, RenderOnce, Styled as _, Window, div};

use crate::theme::{Theme, rem};

const MAX_DISPLAYED: u32 = 999;

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
			.min_w(rem(16.))
			.h(rem(16.))
			.px(rem(4.))
			.rounded_full()
			.bg(theme.mention_badge)
			.text_color(theme.text_on_brand)
			.text_size(theme.font_tiny)
			.font_weight(FontWeight::BOLD)
			.line_height(rem(16.))
			.child(label);
		match self.ring {
			Some(ring) => badge.border_2().border_color(ring).min_w(rem(20.)).h(rem(20.)),
			None => badge,
		}
	}
}
