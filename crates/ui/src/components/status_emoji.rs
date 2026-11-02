use gpui::{
	AnyElement, App, IntoElement as _, ParentElement as _, Rems, SharedString, Styled as _, div, img,
	prelude::FluentBuilder as _,
};
use media::SizeHint;
use protocol::PartialEmoji;

use super::emoji::unicode_emoji;
use crate::images::Images;
use crate::theme::rem_to_pixels;

#[derive(Clone, PartialEq)]
pub enum StatusEmoji {
	Unicode(SharedString),
	Custom { id: u64, animated: bool },
}

impl StatusEmoji {
	fn new(id: Option<u64>, name: Option<&str>, animated: bool) -> Option<Self> {
		match (id, name) {
			(Some(id), _) => Some(Self::Custom { id, animated }),
			(None, Some(name)) if !name.is_empty() => Some(Self::Unicode(name.to_owned().into())),
			_ => None,
		}
	}

	pub fn from_activity(emoji: &PartialEmoji) -> Option<Self> {
		Self::new(emoji.id.map(|id| id.get()), emoji.name.as_deref(), emoji.animated)
	}

	pub fn element(&self, size: Rems, cx: &App) -> AnyElement {
		match self {
			Self::Unicode(glyph) => unicode_emoji(glyph, size, cx),
			Self::Custom { id, animated } => {
				let pixels = (rem_to_pixels(size) * 2.) as u32;
				let url = media::cdn::emoji_url(*id, *animated, pixels);
				let image = cx.global::<Images>().get(&url, SizeHint::square(pixels));
				div()
					.flex_none()
					.size(size)
					.when_some(image, |slot, image| slot.child(img(image).size_full()))
					.into_any_element()
			}
		}
	}
}
