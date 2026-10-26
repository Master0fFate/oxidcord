use std::io::Cursor;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::metadata::{LoopCount, Orientation};
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use crate::error::{MediaError, Result};
use crate::types::{Animation, Decoded, DecodedImage, Frame};

const MIN_FRAME_DELAY_MS: u32 = 20;
const DEFAULT_FRAME_DELAY_MS: u32 = 100;
const BYTES_PER_PIXEL: u64 = 4;
const ALLOC_FACTOR: u64 = 8;

#[derive(Debug, Clone)]
pub struct Limits {
	pub max_pixels: u64,
	pub max_frames: u32,
	pub max_bytes: usize,
	pub max_animation_bytes: usize,
	pub first_frame_only: bool,
}

impl Default for Limits {
	fn default() -> Self {
		Self {
			max_pixels: 32 * 1024 * 1024,
			max_frames: 500,
			max_bytes: 64 * 1024 * 1024,
			max_animation_bytes: 32 * 1024 * 1024,
			first_frame_only: false,
		}
	}
}

pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Decoded> {
	if bytes.len() > limits.max_bytes {
		return Err(MediaError::Limit(format!(
			"{} bytes exceeds {}",
			bytes.len(),
			limits.max_bytes
		)));
	}
	let format =
		image::guess_format(bytes).map_err(|_| MediaError::Decode("unrecognized image format".to_owned()))?;
	let input = Cursor::new(bytes);
	match format {
		ImageFormat::Gif => {
			let mut decoder = GifDecoder::new(input)?;
			let dimensions = prepare(&mut decoder, limits)?;
			collect_frames(decoder, dimensions, limits)
		}
		ImageFormat::WebP => {
			let mut decoder = WebPDecoder::new(input)?;
			let dimensions = prepare(&mut decoder, limits)?;
			if decoder.has_animation() {
				collect_frames(decoder, dimensions, limits)
			} else {
				decode_still(decoder).map(Decoded::Still)
			}
		}
		ImageFormat::Png => {
			let mut decoder = PngDecoder::new(input)?;
			let dimensions = prepare(&mut decoder, limits)?;
			if decoder.is_apng()? {
				collect_frames(decoder.apng()?, dimensions, limits)
			} else {
				decode_still(decoder).map(Decoded::Still)
			}
		}
		ImageFormat::Jpeg | ImageFormat::Bmp | ImageFormat::Ico => {
			let mut decoder = ImageReader::with_format(input, format).into_decoder()?;
			prepare(&mut decoder, limits)?;
			decode_still(decoder).map(Decoded::Still)
		}
		other => Err(MediaError::Decode(format!("unsupported format {other:?}"))),
	}
}

fn prepare(decoder: &mut impl ImageDecoder, limits: &Limits) -> Result<(u32, u32)> {
	let (width, height) = decoder.dimensions();
	let pixels = u64::from(width) * u64::from(height);
	if pixels == 0 || pixels > limits.max_pixels {
		return Err(MediaError::Limit(format!(
			"{width}x{height} exceeds {} pixels",
			limits.max_pixels
		)));
	}
	let mut decoder_limits = image::Limits::default();
	decoder_limits.max_alloc = Some(limits.max_pixels.saturating_mul(ALLOC_FACTOR));
	decoder.set_limits(decoder_limits)?;
	Ok((width, height))
}

fn decode_still(mut decoder: impl ImageDecoder) -> Result<DecodedImage> {
	let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
	let mut image = DynamicImage::from_decoder(decoder)?;
	image.apply_orientation(orientation);
	Ok(into_decoded(image.into_rgba8()))
}

fn collect_frames<'a>(
	decoder: impl AnimationDecoder<'a>,
	(width, height): (u32, u32),
	limits: &Limits,
) -> Result<Decoded> {
	let loops = match decoder.loop_count() {
		LoopCount::Infinite => None,
		LoopCount::Finite(count) => Some(count.get()),
	};
	let frame_bytes = u64::from(width) * u64::from(height) * BYTES_PER_PIXEL;
	let max_kept = (limits.max_animation_bytes as u64 / frame_bytes.max(1)).max(1) as usize;
	let mut frames: Vec<Frame> = Vec::new();
	for frame in decoder.into_frames() {
		let frame = frame?;
		if frames.len() >= limits.max_frames as usize {
			break;
		}
		let delay_ms = frame_delay_ms(frame.delay().into());
		frames.push(Frame {
			image: into_decoded(frame.into_buffer()),
			delay_ms,
		});
		if limits.first_frame_only {
			break;
		}
		if frames.len() >= max_kept.saturating_mul(2) {
			decimate(&mut frames);
		}
	}
	if !limits.first_frame_only {
		while frames.len() > max_kept {
			decimate(&mut frames);
		}
	}
	match frames.len() {
		0 => Err(MediaError::Decode("animation has no frames".to_owned())),
		1 => Ok(Decoded::Still(frames.remove(0).image)),
		_ => Ok(Decoded::Animated(Animation { frames, loops })),
	}
}

fn decimate(frames: &mut Vec<Frame>) {
	let mut kept: Vec<Frame> = Vec::with_capacity(frames.len() / 2 + 1);
	for (index, frame) in std::mem::take(frames).into_iter().enumerate() {
		if index % 2 == 0 {
			kept.push(frame);
		} else if let Some(last) = kept.last_mut() {
			last.delay_ms = last.delay_ms.saturating_add(frame.delay_ms);
		}
	}
	*frames = kept;
}

fn frame_delay_ms(delay: Duration) -> u32 {
	let millis = u32::try_from(delay.as_millis()).unwrap_or(u32::MAX);
	if millis < MIN_FRAME_DELAY_MS {
		DEFAULT_FRAME_DELAY_MS
	} else {
		millis
	}
}

fn into_decoded(buffer: image::RgbaImage) -> DecodedImage {
	DecodedImage {
		width: buffer.width(),
		height: buffer.height(),
		rgba: buffer.into_raw(),
	}
}
