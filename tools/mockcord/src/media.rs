use std::hash::{DefaultHasher, Hash, Hasher};

use axum::extract::Path;
use axum::http::header::{ACCEPT_RANGES, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};

const WIDTH: u32 = 400;
const HEIGHT: u32 = 300;
const GRID_STEP: u32 = 50;
const GRID_LIFT: u8 = 28;

const ASSETS: [(&str, &str, &[u8]); 6] = [
	("clip.mp4", "video/mp4", include_bytes!("../assets/clip.mp4")),
	("clip.webm", "video/webm", include_bytes!("../assets/clip.webm")),
	("gifv.mp4", "video/mp4", include_bytes!("../assets/gifv.mp4")),
	("loop.gif", "image/gif", include_bytes!("../assets/loop.gif")),
	("song.mp3", "audio/mpeg", include_bytes!("../assets/song.mp3")),
	("voice.ogg", "audio/ogg", include_bytes!("../assets/voice.ogg")),
];

pub fn asset(name: &str) -> Option<(&'static str, &'static [u8])> {
	ASSETS
		.iter()
		.find(|(file, _, _)| name == *file || name.ends_with(&format!("-{file}")))
		.map(|(_, kind, bytes)| (*kind, *bytes))
}

fn requested_range(headers: &HeaderMap, len: usize) -> Option<(usize, usize)> {
	let value = headers.get(RANGE)?.to_str().ok()?;
	let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
	let start: usize = start.parse().ok()?;
	let end = end.parse::<usize>().map_or(len - 1, |end| end.min(len - 1));
	(start <= end).then_some((start, end))
}

fn serve_asset(kind: &'static str, bytes: &'static [u8], headers: &HeaderMap) -> Response {
	match requested_range(headers, bytes.len()) {
		Some((start, end)) => (
			StatusCode::PARTIAL_CONTENT,
			[
				(CONTENT_TYPE, kind.to_owned()),
				(ACCEPT_RANGES, "bytes".to_owned()),
				(CONTENT_RANGE, format!("bytes {start}-{end}/{}", bytes.len())),
			],
			&bytes[start..=end],
		)
			.into_response(),
		None => (
			[
				(CONTENT_TYPE, kind.to_owned()),
				(ACCEPT_RANGES, "bytes".to_owned()),
			],
			bytes,
		)
			.into_response(),
	}
}

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

pub async fn media(Path(name): Path<String>, headers: HeaderMap) -> Response {
	if let Some((kind, bytes)) = asset(&name) {
		return serve_asset(kind, bytes, &headers);
	}
	let image = render(seed_of(&name));
	let mut encoded = Vec::new();
	let written =
		PngEncoder::new(&mut encoded).write_image(image.as_raw(), WIDTH, HEIGHT, ExtendedColorType::Rgb8);
	if let Err(error) = written {
		tracing::warn!(%error, "could not encode the mock image");
	}
	([(CONTENT_TYPE, "image/png")], encoded).into_response()
}
