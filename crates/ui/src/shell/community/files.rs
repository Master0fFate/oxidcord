use std::io::Cursor;
use std::sync::Arc;

use gpui::{Context, Image, ImageFormat};
use image::imageops::FilterType;

pub struct Picked {
	pub name: String,
	pub bytes: Vec<u8>,
	pub preview: Arc<Image>,
}

fn format_of(name: &str) -> ImageFormat {
	match name
		.rsplit('.')
		.next()
		.unwrap_or_default()
		.to_ascii_lowercase()
		.as_str()
	{
		"jpg" | "jpeg" => ImageFormat::Jpeg,
		"gif" => ImageFormat::Gif,
		"webp" => ImageFormat::Webp,
		_ => ImageFormat::Png,
	}
}

pub fn square_png(bytes: &[u8], side: u32) -> Option<Vec<u8>> {
	let decoded = image::load_from_memory(bytes).ok()?;
	let resized = decoded.resize_to_fill(side, side, FilterType::Lanczos3);
	let mut encoded = Cursor::new(Vec::new());
	resized.write_to(&mut encoded, image::ImageFormat::Png).ok()?;
	Some(encoded.into_inner())
}

pub fn pick_image<V: 'static>(
	cx: &mut Context<V>,
	side: Option<u32>,
	on_picked: impl Fn(&mut V, Picked, &mut Context<V>) + 'static,
) {
	cx.spawn(async move |this, cx| {
		let Some(handle) = rfd::AsyncFileDialog::new()
			.add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "apng"])
			.pick_file()
			.await
		else {
			return;
		};
		let name = handle.file_name();
		let raw = handle.read().await;
		let bytes = match side {
			Some(side) => {
				cx.background_executor()
					.spawn(async move { square_png(&raw, side) })
					.await
			}
			None => Some(raw),
		};
		let Some(bytes) = bytes else {
			return;
		};
		let format = if side.is_some() {
			ImageFormat::Png
		} else {
			format_of(&name)
		};
		let preview = Arc::new(Image::from_bytes(format, bytes.clone()));
		this.update(cx, |this, cx| {
			on_picked(this, Picked { name, bytes, preview }, cx)
		})
		.ok();
	})
	.detach();
}
