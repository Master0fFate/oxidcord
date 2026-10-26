use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use gpui::{
	AnyElement, App, InteractiveElement as _, IntoElement as _, ObjectFit, ParentElement as _, Rems,
	RenderImage, Styled as _, StyledImage as _, div, img,
};
use media::SizeHint;

use crate::images::Images;
use crate::theme::{Theme, rem_to_pixels};

fn hint(width: Rems, height: Rems) -> SizeHint {
	SizeHint::new(
		rem_to_pixels(width).round() as u32,
		rem_to_pixels(height).round() as u32,
	)
}

fn frame_id(url: &str) -> (&'static str, usize) {
	let mut hasher = DefaultHasher::new();
	url.hash(&mut hasher);
	("cover-image", hasher.finish() as usize)
}

fn framed(image: Option<Arc<RenderImage>>, url: &str, fit: ObjectFit, cx: &App) -> AnyElement {
	div()
		.size_full()
		.bg(cx.global::<Theme>().background_secondary)
		.children(image.map(|image| img(image).id(frame_id(url)).size_full().object_fit(fit)))
		.into_any_element()
}

pub fn cover(url: &str, width: Rems, height: Rems, fit: ObjectFit, cx: &App) -> AnyElement {
	framed(cx.global::<Images>().get(url, hint(width, height)), url, fit, cx)
}

pub fn cover_still(url: &str, width: Rems, height: Rems, fit: ObjectFit, cx: &App) -> AnyElement {
	framed(
		cx.global::<Images>().get_still(url, hint(width, height)),
		url,
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
	let image = images.get(url, hint).or_else(|| images.get_still(still, hint));
	framed(image, url, fit, cx)
}

pub fn sticker_url(sticker: &protocol::Sticker) -> Option<String> {
	let format = media::cdn::StickerFormat::from(i64::from(sticker.format_type?));
	media::cdn::sticker_url(sticker.id.get(), format)
}
