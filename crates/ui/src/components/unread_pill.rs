use std::time::Duration;

use gpui::{
	Animation, AnimationExt as _, App, ElementId, IntoElement, ParentElement as _, Rems, RenderOnce,
	Styled as _, Window, div, ease_out_quint,
};

use crate::theme::{Theme, rem};

const SLIDE: Duration = Duration::from_millis(160);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PillState {
	Hidden,
	Unread,
	Hovered,
	Selected,
}

impl PillState {
	fn key(self) -> &'static str {
		match self {
			Self::Hidden => "hidden",
			Self::Unread => "unread",
			Self::Hovered => "hovered",
			Self::Selected => "selected",
		}
	}

	fn height(self) -> f32 {
		match self {
			Self::Hidden => 0.,
			Self::Unread => 8.,
			Self::Hovered => 20.,
			Self::Selected => 40.,
		}
	}
}

#[derive(IntoElement)]
pub struct UnreadPill {
	id: ElementId,
	from: PillState,
	to: PillState,
	inset: Rems,
}

impl UnreadPill {
	pub fn new(id: impl Into<ElementId>, from: PillState, to: PillState) -> Self {
		Self {
			id: id.into(),
			from,
			to,
			inset: rem(0.),
		}
	}

	pub fn inset(mut self, inset: Rems) -> Self {
		self.inset = inset;
		self
	}
}

impl RenderOnce for UnreadPill {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let color = cx.global::<Theme>().unread_pill;
		let (from, to) = (self.from.height(), self.to.height());
		div()
			.absolute()
			.left(self.inset)
			.top_0()
			.bottom_0()
			.flex()
			.items_center()
			.child(
				div()
					.w(rem(4.))
					.h(rem(to))
					.rounded_r(rem(4.))
					.bg(color)
					.with_animation(
						(self.id, self.to.key()),
						Animation::new(SLIDE).with_easing(ease_out_quint()),
						move |pill, delta| pill.h(rem(from + (to - from) * delta)),
					),
			)
	}
}
