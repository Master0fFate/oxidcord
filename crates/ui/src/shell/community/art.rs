use gpui::{
	AnyElement, App, IntoElement as _, ObjectFit, ParentElement as _, Rems, Styled as _, StyledImage as _,
	div, img,
};
use media::SizeHint;

use crate::images::Images;
use crate::theme::Theme;

pub fn cover(url: &str, width: Rems, height: Rems, fit: ObjectFit, cx: &App) -> AnyElement {
	let hint = SizeHint::new((width.0 * 32.) as u32, (height.0 * 32.) as u32);
	let shown = cx
		.global::<Images>()
		.get(url, hint)
		.map(|image| img(image).size_full().object_fit(fit));
	div()
		.size_full()
		.bg(cx.global::<Theme>().background_secondary)
		.children(shown)
		.into_any_element()
}

pub fn sticker_url(sticker: &protocol::Sticker) -> Option<String> {
	let format = media::cdn::StickerFormat::from(i64::from(sticker.format_type?));
	media::cdn::sticker_url(sticker.id.get(), format)
}
