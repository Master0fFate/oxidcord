use std::io::Cursor;
use std::sync::Arc;

use gpui::{Context, Image, ImageFormat};
use image::AnimationDecoder as _;
use image::codecs::gif::GifDecoder;
use image::imageops::FilterType;

const MAX_ANIMATED_BYTES: usize = 256 * 1024;
pub const IMAGE_TYPES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "apng"];
pub const STICKER_TYPES: &[&str] = &["png", "apng", "gif"];

pub struct Picked {
	pub name: String,
	pub bytes: Vec<u8>,
	pub preview: Arc<Image>,
}

#[derive(Clone, Copy)]
pub enum Fit {
	Original,
	Square(u32),
	SquareUnlessAnimated(u32),
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

fn fitted(raw: Vec<u8>, name: &str, fit: Fit) -> Option<(Vec<u8>, ImageFormat)> {
	match fit {
		Fit::SquareUnlessAnimated(_) if animated_gif(&raw) => Some((raw, ImageFormat::Gif)),
		Fit::Square(side) | Fit::SquareUnlessAnimated(side) => {
			square_png(&raw, side).map(|bytes| (bytes, ImageFormat::Png))
		}
		Fit::Original => Some((raw, format_of(name))),
	}
}

fn animated_gif(bytes: &[u8]) -> bool {
	bytes.len() <= MAX_ANIMATED_BYTES
		&& GifDecoder::new(Cursor::new(bytes)).is_ok_and(|decoder| decoder.into_frames().take(2).count() == 2)
}

pub fn pick_image<V: 'static>(
	cx: &mut Context<V>,
	fit: Fit,
	types: &'static [&'static str],
	on_picked: impl Fn(&mut V, Picked, &mut Context<V>) + 'static,
) {
	cx.spawn(async move |this, cx| {
		let Some(handle) = rfd::AsyncFileDialog::new()
			.add_filter("Images", types)
			.pick_file()
			.await
		else {
			return;
		};
		let name = handle.file_name();
		let path = handle.path().to_path_buf();
		let prepared = cx
			.background_executor()
			.spawn({
				let name = name.clone();
				async move {
					let raw = std::fs::read(path)
						.inspect_err(|error| tracing::warn!(%error, "could not read the picked image"))
						.ok()?;
					fitted(raw, &name, fit)
				}
			})
			.await;
		let Some((bytes, format)) = prepared else {
			return;
		};
		let preview = Arc::new(Image::from_bytes(format, bytes.clone()));
		this.update(cx, |this, cx| {
			on_picked(this, Picked { name, bytes, preview }, cx)
		})
		.ok();
	})
	.detach();
}
