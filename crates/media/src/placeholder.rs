use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD_INDIFFERENT;

use crate::error::{MediaError, Result};
use crate::resize::resize_to_fit;
use crate::types::DecodedImage;

pub fn decode_thumbhash(hash: &[u8]) -> Result<DecodedImage> {
	let (width, height, rgba) = ::thumbhash::thumb_hash_to_rgba(hash)
		.map_err(|()| MediaError::Decode("invalid thumbhash".to_owned()))?;
	Ok(DecodedImage {
		width: width as u32,
		height: height as u32,
		rgba,
	})
}

pub fn placeholder_from_discord(placeholder: &str, width: u32, height: u32) -> Result<DecodedImage> {
	let hash = STANDARD_NO_PAD_INDIFFERENT
		.decode(placeholder)
		.map_err(|error| MediaError::Decode(format!("invalid placeholder encoding: {error}")))?;
	let image = decode_thumbhash(&hash)?;
	Ok(resize_to_fit(&image, width, height))
}
