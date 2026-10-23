use std::hash::{DefaultHasher, Hash, Hasher};

use axum::extract::Path;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};

const WIDTH: u32 = 400;
const HEIGHT: u32 = 300;
const GRID_STEP: u32 = 50;
const GRID_LIFT: u8 = 28;

fn seed_of(name: &str) -> u64 {
	let mut hasher = DefaultHasher::new();
	name.hash(&mut hasher);
	hasher.finish()
}

fn lerp(from: u8, to: u8, amount: f32) -> u8 {
	(f32::from(from) + (f32::from(to) - f32::from(from)) * amount) as u8
}

fn render(seed: u64) -> RgbImage {
	let bytes = seed.to_le_bytes();
	let from = [bytes[0], bytes[1], bytes[2]];
	let to = [bytes[3], bytes[4], bytes[5]];
	let span = (WIDTH + HEIGHT) as f32;
	RgbImage::from_fn(WIDTH, HEIGHT, |x, y| {
		let amount = (x + y) as f32 / span;
		let on_grid = x % GRID_STEP == 0 || y % GRID_STEP == 0;
		let lift = if on_grid { GRID_LIFT } else { 0 };
		Rgb([0, 1, 2].map(|channel| lerp(from[channel], to[channel], amount).saturating_add(lift)))
	})
}

pub async fn media(Path(name): Path<String>) -> impl IntoResponse {
	let image = render(seed_of(&name));
	let mut encoded = Vec::new();
	let written =
		PngEncoder::new(&mut encoded).write_image(image.as_raw(), WIDTH, HEIGHT, ExtendedColorType::Rgb8);
	if let Err(error) = written {
		tracing::warn!(%error, "could not encode the mock image");
	}
	([(CONTENT_TYPE, "image/png")], encoded)
}
