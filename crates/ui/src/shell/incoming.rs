use std::io::Cursor;
use std::path::PathBuf;

use gpui::{ClipboardEntry, ClipboardItem, ImageFormat};

const MAX_UPLOAD_BYTES: u64 = 500 * 1024 * 1024;

pub struct Incoming {
	pub name: String,
	pub bytes: Vec<u8>,
}

pub enum ClipboardImage {
	Encoded(Incoming),
	Bitmap(platform::clipboard::RgbaImage),
}

impl ClipboardImage {
	pub fn into_incoming(self) -> Option<Incoming> {
		match self {
			Self::Encoded(file) => Some(file),
			Self::Bitmap(bitmap) => encode_png(bitmap).map(|bytes| Incoming {
				name: pasted_name(ImageFormat::Png),
				bytes,
			}),
		}
	}
}

pub fn clipboard_paths(item: &ClipboardItem) -> Vec<PathBuf> {
	let listed = item.entries().iter().find_map(|entry| match entry {
		ClipboardEntry::ExternalPaths(paths) => Some(paths.paths().to_vec()),
		_ => None,
	});
	match listed {
		Some(paths) => paths,
		None if cfg!(target_os = "linux") => platform::clipboard::read_files()
			.inspect_err(|error| tracing::warn!(%error, "could not read the clipboard files"))
			.unwrap_or_default()
			.into_iter()
			.filter(|path| path.exists())
			.collect(),
		None => Vec::new(),
	}
}

pub fn paths_only(item: &ClipboardItem) -> bool {
	let entries = item.entries();
	entries
		.iter()
		.any(|entry| matches!(entry, ClipboardEntry::ExternalPaths(_)))
		&& !entries
			.iter()
			.any(|entry| matches!(entry, ClipboardEntry::String(_)))
}

pub fn clipboard_image(item: &ClipboardItem) -> Option<ClipboardImage> {
	let image = item.entries().iter().find_map(|entry| match entry {
		ClipboardEntry::Image(image) => Some(image),
		_ => None,
	})?;
	if uploadable(image.format) {
		return Some(ClipboardImage::Encoded(Incoming {
			name: pasted_name(image.format),
			bytes: image.bytes.clone(),
		}));
	}
	platform::clipboard::read_image()
		.inspect_err(|error| tracing::warn!(%error, "could not read the clipboard image"))
		.ok()
		.flatten()
		.map(ClipboardImage::Bitmap)
}

pub fn read_paths(paths: Vec<PathBuf>, limit: usize) -> Vec<Incoming> {
	paths
		.into_iter()
		.filter_map(|path| {
			let metadata = std::fs::metadata(&path).ok()?;
			if !metadata.is_file() {
				return None;
			}
			if metadata.len() > MAX_UPLOAD_BYTES {
				tracing::warn!(bytes = metadata.len(), "skipped a file over the upload limit");
				return None;
			}
			let name = path.file_name()?.to_string_lossy().into_owned();
			std::fs::read(&path)
				.inspect_err(|error| tracing::warn!(%error, "could not read the attached file"))
				.ok()
				.map(|bytes| Incoming { name, bytes })
		})
		.take(limit)
		.collect()
}

fn uploadable(format: ImageFormat) -> bool {
	matches!(
		format,
		ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp
	)
}

fn pasted_name(format: ImageFormat) -> String {
	format!("pasted-{}.{}", chrono::Utc::now().timestamp(), format.extension())
}

fn encode_png(bitmap: platform::clipboard::RgbaImage) -> Option<Vec<u8>> {
	let width = u32::try_from(bitmap.width).ok()?;
	let height = u32::try_from(bitmap.height).ok()?;
	let buffer = image::RgbaImage::from_raw(width, height, bitmap.bytes)?;
	let mut encoded = Cursor::new(Vec::new());
	buffer
		.write_to(&mut encoded, image::ImageFormat::Png)
		.inspect_err(|error| tracing::warn!(%error, "could not encode the pasted image"))
		.ok()?;
	Some(encoded.into_inner())
}
