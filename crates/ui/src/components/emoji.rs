use gpui::{AnyElement, App, IntoElement, ParentElement as _, Rems, Styled as _, div, img};
use media::SizeHint;

use crate::images::Images;
use crate::theme::rem_to_pixels;

pub fn unicode_emoji(glyph: &str, size: Rems, cx: &App) -> AnyElement {
	let slot = div().flex_none().size(size);
	let pixels = (rem_to_pixels(size) * 2.) as u32;
	let image =
		media::twemoji::url(glyph).and_then(|url| cx.global::<Images>().get(&url, SizeHint::square(pixels)));
	match image {
		Some(image) => slot.child(img(image).size_full()).into_any_element(),
		None => slot
			.text_size(size * 0.9)
			.line_height(size)
			.child(glyph.to_owned())
			.into_any_element(),
	}
}
