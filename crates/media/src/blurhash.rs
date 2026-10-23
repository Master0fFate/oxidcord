use crate::error::{MediaError, Result};
use crate::types::DecodedImage;

pub fn decode(hash: &str, width: u32, height: u32) -> Result<DecodedImage> {
	let rgba = ::blurhash::decode(hash, width, height, 1.0)
		.map_err(|error| MediaError::Decode(error.to_string()))?;
	Ok(DecodedImage { width, height, rgba })
}
