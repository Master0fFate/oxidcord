use gpui::{App, Hsla, IntoElement, ParentElement as _, Rems, RenderOnce, Styled as _, Window, div};

use crate::model::Status;
use crate::theme::{Theme, rem};

pub fn status_color(theme: &Theme, status: &Status) -> Hsla {
	match status {
		Status::Online => theme.status_online,
		Status::Idle => theme.status_idle,
		Status::Dnd => theme.status_dnd,
		_ => theme.status_offline,
	}
}

#[derive(IntoElement)]
pub struct StatusDot {
	status: Status,
	size: Rems,
	cutout: Option<Hsla>,
}

impl StatusDot {
	pub fn new(status: Status) -> Self {
		Self {
			status,
			size: rem(10.),
			cutout: None,
		}
	}

	pub fn size(mut self, size: Rems) -> Self {
		self.size = size;
		self
	}

	pub fn cutout(mut self, color: Hsla) -> Self {
		self.cutout = Some(color);
		self
	}
}

impl RenderOnce for StatusDot {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let color = status_color(theme, &self.status);
		let cutout = self.cutout.unwrap_or(theme.background_secondary);
		let size = self.size;
		let dot = div()
			.relative()
			.flex_none()
			.size(size)
			.rounded(theme.round(size))
			.bg(color);
		match self.status {
			Status::Online => dot,
			Status::Idle => dot.child(
				div()
					.absolute()
					.size(size * 0.7)
					.top(-size * 0.2)
					.left(-size * 0.2)
					.rounded(theme.round(size * 0.7))
					.bg(cutout),
			),
			Status::Dnd => dot.flex().items_center().justify_center().child(
				div()
					.w(size * 0.64)
					.h(size * 0.26)
					.rounded(size * 0.13)
					.bg(cutout),
			),
			_ => dot
				.flex()
				.items_center()
				.justify_center()
				.child(div().size(size * 0.5).rounded(theme.round(size * 0.5)).bg(cutout)),
		}
	}
}
