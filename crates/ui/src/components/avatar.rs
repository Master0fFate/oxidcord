use gpui::{
	App, FontWeight, Hsla, IntoElement, ObjectFit, ParentElement as _, Rems, RenderOnce, SharedString,
	Styled as _, StyledImage as _, Window, div, img, prelude::FluentBuilder as _,
};
use media::SizeHint;

use super::status_dot::StatusDot;
use crate::images::Images;
use crate::model::Status;
use crate::theme::{Theme, rem};

#[derive(IntoElement)]
pub struct Avatar {
	name: SharedString,
	size: Rems,
	url: Option<SharedString>,
	color: Option<Hsla>,
	status: Option<Status>,
	cutout: Option<Hsla>,
}

impl Avatar {
	pub fn new(name: impl Into<SharedString>) -> Self {
		Self {
			name: name.into(),
			size: rem(32.),
			url: None,
			color: None,
			status: None,
			cutout: None,
		}
	}

	pub fn size(mut self, size: Rems) -> Self {
		self.size = size;
		self
	}

	pub fn url(mut self, url: impl Into<SharedString>) -> Self {
		self.url = Some(url.into());
		self
	}

	pub fn color(mut self, color: Hsla) -> Self {
		self.color = Some(color);
		self
	}

	pub fn status(mut self, status: Status) -> Self {
		self.status = Some(status);
		self
	}

	pub fn cutout(mut self, color: Hsla) -> Self {
		self.cutout = Some(color);
		self
	}

	fn initials(&self) -> String {
		self.name
			.chars()
			.find(|c| c.is_alphanumeric())
			.map(|c| c.to_uppercase().collect())
			.unwrap_or_default()
	}
}

impl RenderOnce for Avatar {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let background = self.color.unwrap_or(theme.brand);
		let text_color = theme.text_on_brand;
		let cutout = self.cutout.unwrap_or(theme.background_secondary);
		let size = self.size;
		let initials = self.initials();
		let base = size.0 * 16.;
		let outer_base = (base * 0.5).clamp(10., 24.);
		let outer = rem(outer_base);
		let ring = rem((outer_base * 0.19).round());
		let pixels = (base * 2.).round() as u32;
		let image = self
			.url
			.as_ref()
			.and_then(|url| cx.global::<Images>().get(url, SizeHint::square(pixels)));
		let circle = div().size(size).rounded_full().overflow_hidden().flex_none();
		let circle = match image {
			Some(image) => circle.child(img(image).size_full().rounded_full().object_fit(ObjectFit::Cover)),
			None => circle
				.bg(background)
				.flex()
				.items_center()
				.justify_center()
				.text_color(text_color)
				.font_weight(FontWeight::SEMIBOLD)
				.text_size(size * 0.42)
				.child(initials),
		};
		div()
			.relative()
			.flex_none()
			.size(size)
			.child(circle)
			.when_some(self.status, |avatar, status| {
				avatar.child(
					div()
						.absolute()
						.right(-ring)
						.bottom(-ring)
						.size(outer)
						.rounded_full()
						.bg(cutout)
						.flex()
						.items_center()
						.justify_center()
						.child(StatusDot::new(status).size(outer - ring * 2.).cutout(cutout)),
				)
			})
	}
}
