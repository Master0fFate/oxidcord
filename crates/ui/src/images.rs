use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use gpui::{Global, RenderImage};
use image::{Delay, Frame, RgbaImage};
use lru::LruCache;
use media::{Decoded, DecodedImage, MediaService, SizeHint};
use smallvec::SmallVec;
use url::Url;

const RENDERED_CAPACITY: usize = 512;
const PLACEHOLDER_CAPACITY: usize = 256;

static UI_SCALE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

pub fn set_ui_scale(scale: f32) {
	UI_SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

pub fn scaled_pixels(pixels: u32) -> u32 {
	let scale = f32::from_bits(UI_SCALE.load(Ordering::Relaxed));
	if scale > 1. {
		(pixels as f32 * scale).ceil() as u32
	} else {
		pixels
	}
}

fn scaled_hint(hint: SizeHint) -> SizeHint {
	if hint == SizeHint::ORIGINAL {
		hint
	} else {
		SizeHint::new(scaled_pixels(hint.width), scaled_pixels(hint.height))
	}
}

struct Rendered {
	source: Arc<Decoded>,
	image: Arc<RenderImage>,
}

pub struct Images {
	media: MediaService,
	mock_origin: Option<Url>,
	rendered: RefCell<LruCache<(String, SizeHint), Rendered>>,
	placeholders: RefCell<LruCache<String, Arc<RenderImage>>>,
}

impl Global for Images {}

fn capacity(items: usize) -> NonZeroUsize {
	NonZeroUsize::new(items).expect("capacity is not zero")
}

fn bgra(image: &DecodedImage) -> Option<RgbaImage> {
	let mut buffer = RgbaImage::from_raw(image.width, image.height, image.rgba.clone())?;
	for pixel in buffer.pixels_mut() {
		pixel.0.swap(0, 2);
	}
	Some(buffer)
}

fn render_image(decoded: &Decoded) -> Option<Arc<RenderImage>> {
	let frames: SmallVec<[Frame; 1]> = match decoded {
		Decoded::Still(image) => smallvec::smallvec![Frame::new(bgra(image)?)],
		Decoded::Animated(animation) => animation
			.frames
			.iter()
			.filter_map(|frame| {
				let delay = Delay::from_saturating_duration(Duration::from_millis(u64::from(frame.delay_ms)));
				Some(Frame::from_parts(bgra(&frame.image)?, 0, 0, delay))
			})
			.collect(),
	};
	(!frames.is_empty()).then(|| Arc::new(RenderImage::new(frames)))
}

impl Images {
	pub fn new(media: MediaService, mock_origin: Option<Url>) -> Self {
		Self {
			media,
			mock_origin,
			rendered: RefCell::new(LruCache::new(capacity(RENDERED_CAPACITY))),
			placeholders: RefCell::new(LruCache::new(capacity(PLACEHOLDER_CAPACITY))),
		}
	}

	pub fn get(&self, url: &str, hint: SizeHint) -> Option<Arc<RenderImage>> {
		let hint = scaled_hint(hint);
		let url = self.resolve(url);
		let Some(decoded) = self.media.peek(&url, hint) else {
			self.media.request(&url, hint);
			return None;
		};
		let mut rendered = self.rendered.borrow_mut();
		let key = (url, hint);
		if let Some(entry) = rendered.get(&key)
			&& Arc::ptr_eq(&entry.source, &decoded)
		{
			return Some(entry.image.clone());
		}
		let image = render_image(&decoded)?;
		rendered.put(
			key,
			Rendered {
				source: decoded,
				image: image.clone(),
			},
		);
		Some(image)
	}

	pub fn placeholder(&self, hash: &str, width: u32, height: u32) -> Option<Arc<RenderImage>> {
		let mut placeholders = self.placeholders.borrow_mut();
		if let Some(image) = placeholders.get(hash) {
			return Some(image.clone());
		}
		let decoded = media::placeholder_from_discord(hash, width, height).ok()?;
		let image = render_image(&Decoded::Still(decoded))?;
		placeholders.put(hash.to_owned(), image.clone());
		Some(image)
	}

	fn resolve(&self, url: &str) -> String {
		let Some(origin) = &self.mock_origin else {
			return url.to_owned();
		};
		let Ok(parsed) = Url::parse(url) else {
			return url.to_owned();
		};
		if parsed.host_str() == origin.host_str() {
			return url.to_owned();
		}
		let mut hasher = DefaultHasher::new();
		parsed.path().hash(&mut hasher);
		format!("{}media/{:016x}.png", origin, hasher.finish())
	}
}

pub fn from_rgba(mut buffer: RgbaImage) -> Arc<RenderImage> {
	for pixel in buffer.pixels_mut() {
		pixel.0.swap(0, 2);
	}
	Arc::new(RenderImage::new(smallvec::smallvec![Frame::new(buffer)]))
}
