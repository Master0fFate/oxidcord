use gpui::{
	AnyElement, App, IntoElement as _, ObjectFit, ParentElement as _, Rems, Styled as _, StyledImage as _,
	div, img,
};
use media::SizeHint;

use crate::images::{Images, Picture};
use crate::theme::{Theme, rem_to_pixels};

fn hint(width: Rems, height: Rems) -> SizeHint {
	SizeHint::new(
		rem_to_pixels(width).round() as u32,
		rem_to_pixels(height).round() as u32,
	)
}

fn framed(image: Option<Picture>, fit: ObjectFit, cx: &App) -> AnyElement {
	div()
		.size_full()
		.bg(cx.global::<Theme>().background_secondary)
		.children(image.map(|image| img(image).size_full().object_fit(fit)))
		.into_any_element()
}

pub fn cover(url: &str, width: Rems, height: Rems, fit: ObjectFit, cx: &App) -> AnyElement {
	framed(cx.global::<Images>().get(url, hint(width, height)), fit, cx)
}

pub fn cover_still(url: &str, width: Rems, height: Rems, fit: ObjectFit, cx: &App) -> AnyElement {
	framed(
		cx.global::<Images>()
			.get_still(url, hint(width, height))
			.map(Picture::from),
		fit,
		cx,
	)
}

pub fn cover_animated(
	url: &str,
	still: &str,
	width: Rems,
	height: Rems,
	fit: ObjectFit,
	cx: &App,
) -> AnyElement {
	let hint = hint(width, height);
	let images = cx.global::<Images>();
	let image = images
		.get(url, hint)
		.or_else(|| images.get_still(still, hint).map(Picture::from));
	framed(image, fit, cx)
}

pub fn sticker_url(sticker: &protocol::Sticker) -> Option<String> {
	let format = media::cdn::StickerFormat::from(i64::from(sticker.format_type?));
	media::cdn::sticker_url(sticker.id.get(), format)
}
