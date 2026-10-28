use std::borrow::Cow;
use std::path::PathBuf;

use arboard::{Clipboard, ImageData};

use crate::Result;

#[derive(Debug, Clone)]
pub struct RgbaImage {
	pub width: usize,
	pub height: usize,
	pub bytes: Vec<u8>,
}

pub fn read_text() -> Result<Option<String>> {
	missing_as_none(Clipboard::new()?.get_text())
}

pub fn write_text(text: &str) -> Result<()> {
	Ok(Clipboard::new()?.set_text(text)?)
}

pub fn read_files() -> Result<Vec<PathBuf>> {
	Ok(missing_as_none(Clipboard::new()?.get().file_list())?.unwrap_or_default())
}

pub fn read_image() -> Result<Option<RgbaImage>> {
	let image = missing_as_none(Clipboard::new()?.get_image())?;
	Ok(image.map(|image| RgbaImage {
		width: image.width,
		height: image.height,
		bytes: image.bytes.into_owned(),
	}))
}

pub fn write_image(image: &RgbaImage) -> Result<()> {
	Ok(Clipboard::new()?.set_image(ImageData {
		width: image.width,
		height: image.height,
		bytes: Cow::Borrowed(&image.bytes),
	})?)
}

fn missing_as_none<T>(result: std::result::Result<T, arboard::Error>) -> Result<Option<T>> {
	match result {
		Ok(value) => Ok(Some(value)),
		Err(arboard::Error::ContentNotAvailable) => Ok(None),
		Err(error) => Err(error.into()),
	}
}
