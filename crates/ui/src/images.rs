use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use gpui::{App, EntityId, Global, ImageSource, RenderImage, Window};
use image::{Frame, RgbaImage};
use lru::LruCache;
use media::{Decoded, DecodedImage, MediaService, SizeHint};
use url::Url;

const RENDERED_CAPACITY: usize = 512;
const RENDERED_BUDGET_BYTES: usize = 128 * 1024 * 1024;
const PLACEHOLDER_CAPACITY: usize = 256;
const FRAME_GRID_MS: u128 = 50;

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
	picture: Picture,
	bytes: usize,
}

pub struct Clip {
	frames: Vec<Arc<RenderImage>>,
	ends: Vec<Duration>,
	total: Duration,
	loops: Option<u32>,
	started: Instant,
}

impl Clip {
	fn at(&self, now: Instant) -> (Arc<RenderImage>, Option<Duration>) {
		let elapsed = now.duration_since(self.started);
		if let Some(loops) = self.loops
			&& elapsed >= self.total * loops
		{
			return (self.frames[self.frames.len() - 1].clone(), None);
		}
		let phase = Duration::from_nanos((elapsed.as_nanos() % self.total.as_nanos()) as u64);
		let index = self.ends.partition_point(|end| *end <= phase);
		(self.frames[index].clone(), Some(self.ends[index] - phase))
	}
}

#[derive(Clone)]
pub enum Picture {
	Still(Arc<RenderImage>),
	Animated(Arc<Clip>),
}

impl Picture {
	fn frames(&self) -> impl Iterator<Item = &Arc<RenderImage>> {
		match self {
			Self::Still(image) => std::slice::from_ref(image).iter(),
			Self::Animated(clip) => clip.frames.iter(),
		}
	}
}

impl From<Arc<RenderImage>> for Picture {
	fn from(image: Arc<RenderImage>) -> Self {
		Self::Still(image)
	}
}

impl From<Picture> for ImageSource {
	fn from(picture: Picture) -> Self {
		match picture {
			Picture::Still(image) => Self::Render(image),
			Picture::Animated(clip) => Self::Custom(Arc::new(move |window, cx| {
				Some(Ok(Images::frame(&clip, window, cx)))
			})),
		}
	}
}

pub struct Images {
	media: MediaService,
	mock_origin: Option<Url>,
	rendered: RefCell<LruCache<RenderedKey, Rendered>>,
	retained: Cell<usize>,
	placeholders: RefCell<LruCache<String, Arc<RenderImage>>>,
	retired: RefCell<Vec<Arc<RenderImage>>>,
	due: RefCell<HashMap<EntityId, Instant>>,
	ticking: Cell<bool>,
	origin: Instant,
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

fn single(buffer: RgbaImage) -> (Arc<RenderImage>, usize) {
	let bytes = buffer.as_raw().len();
	(
		Arc::new(RenderImage::new(smallvec::smallvec![Frame::new(buffer)])),
		bytes,
	)
}

pub fn render_image(decoded: &Decoded, still: bool) -> Option<(Picture, usize)> {
	let animation = match decoded {
		Decoded::Still(image) => {
			let (image, bytes) = single(bgra(image)?);
			return Some((Picture::Still(image), bytes));
		}
		Decoded::Animated(animation) => animation,
	};
	let mut frames = Vec::with_capacity(animation.frames.len());
	let mut ends = Vec::with_capacity(animation.frames.len());
	let mut total = Duration::ZERO;
	let mut bytes = 0;
	for frame in animation.frames.iter().take(if still { 1 } else { usize::MAX }) {
		let Some(buffer) = bgra(&frame.image) else {
			continue;
		};
		let (image, size) = single(buffer);
		bytes += size;
		total += Duration::from_millis(u64::from(frame.delay_ms));
		ends.push(total);
		frames.push(image);
	}
	if frames.len() < 2 {
		return frames.pop().map(|image| (Picture::Still(image), bytes));
	}
	let clip = Clip {
		frames,
		ends,
		total,
		loops: animation.loops,
		started: Instant::now(),
	};
	Some((Picture::Animated(Arc::new(clip)), bytes))
}

impl Images {
	pub fn new(media: MediaService, mock_origin: Option<Url>) -> Self {
		Self {
			media,
			mock_origin,
			rendered: RefCell::new(LruCache::new(capacity(RENDERED_CAPACITY))),
			retained: Cell::new(0),
			placeholders: RefCell::new(LruCache::new(capacity(PLACEHOLDER_CAPACITY))),
			retired: RefCell::new(Vec::new()),
			due: RefCell::new(HashMap::new()),
			ticking: Cell::new(false),
			origin: Instant::now(),
		}
	}

	pub fn get(&self, url: &str, hint: SizeHint) -> Option<Picture> {
		self.rendered_image(url, hint, false)
	}

	pub fn get_still(&self, url: &str, hint: SizeHint) -> Option<Arc<RenderImage>> {
		match self.rendered_image(url, hint, true)? {
			Picture::Still(image) => Some(image),
			Picture::Animated(clip) => Some(clip.frames[0].clone()),
		}
	}

	fn rendered_image(&self, url: &str, hint: SizeHint, still: bool) -> Option<Picture> {
		let hint = scaled_hint(hint);
		let url = self.resolve(url);
		let key = RenderedKey { url, hint, still };
		let cached = self
			.rendered
			.borrow_mut()
			.get(&key)
			.map(|entry| entry.picture.clone());
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
		let (picture, bytes) = render_image(&decoded, still)?;
		self.store(key, picture.clone(), bytes);
		Some(picture)
	}

	fn store(&self, key: RenderedKey, picture: Picture, bytes: usize) {
		let mut rendered = self.rendered.borrow_mut();
		let mut retained = self.retained.get() + bytes;
		if let Some((_, old)) = rendered.push(key, Rendered { picture, bytes }) {
			retained = retained.saturating_sub(old.bytes);
			self.retire(&old.picture);
		}
		while retained > RENDERED_BUDGET_BYTES && rendered.len() > 1 {
			let Some((_, old)) = rendered.pop_lru() else {
				break;
			};
			retained = retained.saturating_sub(old.bytes);
			self.retire(&old.picture);
		}
		self.retained.set(retained);
	}

	fn retire(&self, picture: &Picture) {
		self.retired.borrow_mut().extend(picture.frames().cloned());
	}

	pub fn release(cx: &mut App) {
		let retired = cx.global::<Self>().retired.take();
		for image in retired {
			cx.drop_image(image, None);
		}
	}

	fn frame(clip: &Clip, window: &Window, cx: &mut App) -> Arc<RenderImage> {
		if cx.reduce_motion() {
			return clip.frames[0].clone();
		}
		let (frame, next) = clip.at(Instant::now());
		if let Some(next) = next
			&& window.is_window_active()
		{
			Self::schedule(window.current_view(), next, cx);
		}
		frame
	}

	fn schedule(view: EntityId, after: Duration, cx: &mut App) {
		let start = {
			let images = cx.global::<Self>();
			let at = images.grid_slot(Instant::now() + after);
			images
				.due
				.borrow_mut()
				.entry(view)
				.and_modify(|due| *due = (*due).min(at))
				.or_insert(at);
			!images.ticking.replace(true)
		};
		if start {
			Self::run_ticker(cx);
		}
	}

	fn grid_slot(&self, at: Instant) -> Instant {
		let ticks = at.duration_since(self.origin).as_millis().div_ceil(FRAME_GRID_MS);
		self.origin + Duration::from_millis((ticks * FRAME_GRID_MS) as u64)
	}

	fn run_ticker(cx: &mut App) {
		cx.spawn(async move |cx| {
			while let Some(at) = cx.update(|cx| cx.global::<Self>().next_due()) {
				cx.background_executor()
					.timer(at.saturating_duration_since(Instant::now()))
					.await;
				cx.update(Self::tick);
			}
		})
		.detach();
	}

	fn next_due(&self) -> Option<Instant> {
		let next = self.due.borrow().values().min().copied();
		if next.is_none() {
			self.ticking.set(false);
		}
		next
	}

	fn tick(cx: &mut App) {
		let now = Instant::now();
		let views: Vec<EntityId> = cx
			.global::<Self>()
			.due
			.borrow_mut()
			.extract_if(|_, at| *at <= now)
			.map(|(view, _)| view)
			.collect();
		for view in views {
			cx.notify(view);
		}
		Self::release(cx);
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
		let (image, _) = single(bgra(&decoded)?);
		if let Some((_, evicted)) = placeholders.push(hash.to_owned(), image.clone()) {
			self.retired.borrow_mut().push(evicted);
		}
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
