use std::cell::{Cell, RefCell};
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
const RENDERED_BUDGET_BYTES: usize = 128 * 1024 * 1024;
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

#[derive(Clone, PartialEq, Eq, Hash)]
struct RenderedKey {
	url: String,
	hint: SizeHint,
	still: bool,
}

struct Rendered {
	image: Arc<RenderImage>,
	bytes: usize,
}

pub struct Images {
	media: MediaService,
	mock_origin: Option<Url>,
	rendered: RefCell<LruCache<RenderedKey, Rendered>>,
	retained: Cell<usize>,
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

fn render_image(decoded: &Decoded, still: bool) -> Option<(Arc<RenderImage>, usize)> {
	let frames: SmallVec<[Frame; 1]> = match decoded {
		Decoded::Still(image) => smallvec::smallvec![Frame::new(bgra(image)?)],
		Decoded::Animated(animation) => animation
			.frames
			.iter()
			.take(if still { 1 } else { usize::MAX })
			.filter_map(|frame| {
				let delay = Delay::from_saturating_duration(Duration::from_millis(u64::from(frame.delay_ms)));
				Some(Frame::from_parts(bgra(&frame.image)?, 0, 0, delay))
			})
			.collect(),
	};
	let bytes = frames
		.iter()
		.map(|frame| frame.buffer().as_raw().len())
		.sum::<usize>();
	(!frames.is_empty()).then(|| (Arc::new(RenderImage::new(frames)), bytes))
}

impl Images {
	pub fn new(media: MediaService, mock_origin: Option<Url>) -> Self {
		Self {
			media,
			mock_origin,
			rendered: RefCell::new(LruCache::new(capacity(RENDERED_CAPACITY))),
			retained: Cell::new(0),
			placeholders: RefCell::new(LruCache::new(capacity(PLACEHOLDER_CAPACITY))),
		}
	}

	pub fn get(&self, url: &str, hint: SizeHint) -> Option<Arc<RenderImage>> {
		self.rendered_image(url, hint, false)
	}

	pub fn get_still(&self, url: &str, hint: SizeHint) -> Option<Arc<RenderImage>> {
		self.rendered_image(url, hint, true)
	}

	fn rendered_image(&self, url: &str, hint: SizeHint, still: bool) -> Option<Arc<RenderImage>> {
		let hint = scaled_hint(hint);
		let url = self.resolve(url);
		let key = RenderedKey { url, hint, still };
		let cached = self
			.rendered
			.borrow_mut()
			.get(&key)
			.map(|entry| entry.image.clone());
		if cached.is_some() {
			return cached;
		}
		if media::cdn::is_video(&key.url) {
			return None;
		}
		let Some(decoded) = self.media.peek(&key.url, hint, still) else {
			self.media.request(&key.url, hint, still);
			return None;
		};
		let (image, bytes) = render_image(&decoded, still)?;
		self.store(key, image.clone(), bytes);
		Some(image)
	}

	fn store(&self, key: RenderedKey, image: Arc<RenderImage>, bytes: usize) {
		let mut rendered = self.rendered.borrow_mut();
		if let Some(old) = rendered.put(key, Rendered { image, bytes }) {
			self.retained.set(self.retained.get().saturating_sub(old.bytes));
		}
		self.retained.set(self.retained.get() + bytes);
		let mut retained = self.retained.get();
		while retained > RENDERED_BUDGET_BYTES && rendered.len() > 1 {
			let Some((_, old)) = rendered.pop_lru() else {
				break;
			};
			retained = retained.saturating_sub(old.bytes);
		}
		self.retained.set(retained);
	}

	pub fn copy_image(&self, url: &str) {
		let url = self.resolve(url);
		let media = self.media.clone();
		self.media.runtime().spawn(async move {
			let copied = async {
				let bytes = media
					.fetcher()
					.fetch(&url)
					.await
					.map_err(|error| error.to_string())?;
				let decoded = image::load_from_memory(&bytes)
					.map_err(|error| error.to_string())?
					.to_rgba8();
				let (width, height) = decoded.dimensions();
				platform::clipboard::write_image(&platform::clipboard::RgbaImage {
					width: width as usize,
					height: height as usize,
					bytes: decoded.into_raw(),
				})
				.map_err(|error| error.to_string())
			};
			if let Err(error) = copied.await {
				tracing::warn!(%error, "could not copy the image");
			}
		});
	}

	pub fn save_image(&self, url: &str, name: String) {
		let url = self.resolve(url);
		let media = self.media.clone();
		self.media.runtime().spawn(async move {
			let Some(handle) = rfd::AsyncFileDialog::new().set_file_name(&name).save_file().await else {
				return;
			};
			let saved = async {
				let bytes = media
					.fetcher()
					.fetch(&url)
					.await
					.map_err(|error| error.to_string())?;
				handle.write(&bytes).await.map_err(|error| error.to_string())
			};
			if let Err(error) = saved.await {
				tracing::warn!(%error, "could not save the image");
			}
		});
	}

	pub fn placeholder(&self, hash: &str, width: u32, height: u32) -> Option<Arc<RenderImage>> {
		let mut placeholders = self.placeholders.borrow_mut();
		if let Some(image) = placeholders.get(hash) {
			return Some(image.clone());
		}
		let decoded = media::placeholder_from_discord(hash, width, height).ok()?;
		let (image, _) = render_image(&Decoded::Still(decoded), true)?;
		placeholders.put(hash.to_owned(), image.clone());
		Some(image)
	}

	fn resolve(&self, url: &str) -> String {
		match &self.mock_origin {
			Some(origin) if !media::twemoji::is_pack_url(url) => mock_url(origin, url),
			_ => url.to_owned(),
		}
	}
}

pub fn mock_url(origin: &Url, url: &str) -> String {
	let Ok(parsed) = Url::parse(url) else {
		return url.to_owned();
	};
	if parsed.host_str() == origin.host_str() {
		return url.to_owned();
	}
	let mut hasher = DefaultHasher::new();
	parsed.path().hash(&mut hasher);
	let file = parsed
		.path_segments()
		.and_then(|mut segments| segments.next_back())
		.filter(|file| file.contains('.'))
		.filter(|file| {
			file.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
		});
	match file {
		Some(file) => format!("{}media/{:016x}-{file}", origin, hasher.finish()),
		None => format!("{}media/{:016x}.png", origin, hasher.finish()),
	}
}

pub fn from_rgba(mut buffer: RgbaImage) -> Arc<RenderImage> {
	for pixel in buffer.pixels_mut() {
		pixel.0.swap(0, 2);
	}
	Arc::new(RenderImage::new(smallvec::smallvec![Frame::new(buffer)]))
}
