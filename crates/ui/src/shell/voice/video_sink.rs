use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gpui::{App, Context, Global, RenderImage, Task};
use image::{Frame, RgbaImage};
use parking_lot::{Condvar, Mutex, RwLock};
use protocol::Snowflake;
use session::{CameraSource, ScreenSource, ScreenTarget, VideoFrame, VideoSink, VideoStream};
use smallvec::smallvec;
use state::VideoKind;
use tokio::sync::watch;
use voice::video::capture::VideoSource;
use voice::video::scaled_bgra;

pub type StreamKey = (Snowflake, VideoKind);

pub const PREVIEW_USER: Snowflake = Snowflake(0);

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const TEXTURE_MAX_SIDE: u32 = 1280;
const THUMBNAIL_MAX_SIDE: u32 = 480;
const PREVIEW_FPS: u32 = 10;
const PREVIEW_CAMERA_WIDTH: u32 = 640;
const PREVIEW_CAMERA_HEIGHT: u32 = 360;
const THUMBNAIL_FPS: u32 = 4;
const THUMBNAIL_ATTEMPTS: usize = 3;

#[derive(Clone)]
pub enum PreviewSource {
	Screen(ScreenTarget),
	Camera(Option<String>),
}

#[derive(Default)]
struct Pending {
	frames: HashMap<StreamKey, Arc<VideoFrame>>,
	ended: Vec<StreamKey>,
	closed: bool,
}

struct Shared {
	pending: Mutex<Pending>,
	wake: Condvar,
	images: RwLock<HashMap<StreamKey, Arc<RenderImage>>>,
	retired: Mutex<Vec<Arc<RenderImage>>>,
	changed: watch::Sender<u64>,
}

struct Preview {
	stop: Arc<AtomicBool>,
}

pub struct VideoTextures {
	shared: Arc<Shared>,
	worker: Mutex<Option<JoinHandle<()>>>,
	previews: Mutex<HashMap<StreamKey, Preview>>,
}

#[derive(Clone)]
pub struct Textures(pub Arc<VideoTextures>);

impl Global for Textures {}

pub fn video_image(cx: &App, user: Snowflake) -> Option<Arc<RenderImage>> {
	let textures = cx.try_global::<Textures>()?;
	textures
		.0
		.image((user, VideoKind::Screen))
		.or_else(|| textures.0.image((user, VideoKind::Camera)))
}

pub fn follow_frames<T: 'static>(
	cx: &mut Context<T>,
	redraw: impl Fn(&mut T, &mut Context<T>) + 'static,
) -> Task<()> {
	let Some(textures) = cx.try_global::<Textures>().cloned() else {
		return Task::ready(());
	};
	let mut frames = textures.0.frames();
	cx.spawn(async move |this, cx| {
		while frames.changed().await.is_ok() {
			let retired = textures.0.take_retired();
			let alive = this
				.update(cx, |view, cx| {
					for image in retired {
						cx.drop_image(image, None);
					}
					redraw(view, cx);
				})
				.is_ok();
			if !alive {
				break;
			}
		}
	})
}

fn ui_kind(kind: session::VideoKind) -> VideoKind {
	match kind {
		session::VideoKind::Camera => VideoKind::Camera,
		session::VideoKind::Screen => VideoKind::Screen,
	}
}

impl Shared {
	fn submit(&self, key: StreamKey, frame: Arc<VideoFrame>) {
		let mut pending = self.pending.lock();
		pending.frames.insert(key, frame);
		self.wake.notify_one();
	}

	fn end(&self, key: StreamKey) {
		let mut pending = self.pending.lock();
		pending.frames.remove(&key);
		pending.ended.push(key);
		self.wake.notify_one();
	}

	fn publish(&self, key: StreamKey, image: Arc<RenderImage>) {
		if let Some(previous) = self.images.write().insert(key, image) {
			self.retired.lock().push(previous);
		}
	}

	fn remove(&self, key: StreamKey) {
		if let Some(previous) = self.images.write().remove(&key) {
			self.retired.lock().push(previous);
		}
	}
}

fn run_worker(shared: Arc<Shared>) {
	let mut last = Instant::now()
		.checked_sub(FRAME_INTERVAL)
		.unwrap_or_else(Instant::now);
	loop {
		{
			let mut pending = shared.pending.lock();
			while pending.frames.is_empty() && pending.ended.is_empty() && !pending.closed {
				shared.wake.wait(&mut pending);
			}
			if pending.closed {
				return;
			}
		}
		let elapsed = last.elapsed();
		if elapsed < FRAME_INTERVAL {
			thread::sleep(FRAME_INTERVAL - elapsed);
		}
		last = Instant::now();
		let (frames, ended) = {
			let mut pending = shared.pending.lock();
			if pending.closed {
				return;
			}
			(
				std::mem::take(&mut pending.frames),
				std::mem::take(&mut pending.ended),
			)
		};
		for key in ended {
			shared.remove(key);
		}
		let mut published = false;
		for (key, frame) in frames {
			if let Some(image) = render_image(&frame, TEXTURE_MAX_SIDE) {
				shared.publish(key, image);
				published = true;
			}
		}
		if published {
			shared
				.changed
				.send_modify(|version| *version = version.wrapping_add(1));
		}
	}
}

fn open_camera(id: Option<String>) -> Result<Box<dyn VideoSource>, voice::video::VideoError> {
	let id = match id {
		Some(id) => id,
		None => CameraSource::list()?
			.into_iter()
			.next()
			.map(|camera| camera.id)
			.ok_or(voice::video::VideoError::NotFound)?,
	};
	CameraSource::open(&id, PREVIEW_CAMERA_WIDTH, PREVIEW_CAMERA_HEIGHT, PREVIEW_FPS)
		.map(|source| Box::new(source) as Box<dyn VideoSource>)
}

fn run_preview(shared: Arc<Shared>, key: StreamKey, source: PreviewSource, stop: Arc<AtomicBool>) {
	let opened: Result<Box<dyn VideoSource>, voice::video::VideoError> = match source {
		PreviewSource::Screen(target) => {
			ScreenSource::open(&target, PREVIEW_FPS).map(|source| Box::new(source) as Box<dyn VideoSource>)
		}
		PreviewSource::Camera(id) => open_camera(id),
	};
	let mut source = match opened {
		Ok(source) => source,
		Err(error) => {
			tracing::warn!(%error, "could not open the video preview");
			return;
		}
	};
	while !stop.load(Ordering::Relaxed) {
		match source.next_frame() {
			Ok(frame) => shared.submit(key, Arc::new(frame)),
			Err(error) => {
				tracing::warn!(%error, "video preview stopped");
				break;
			}
		}
	}
	shared.end(key);
}

impl VideoTextures {
	pub fn new() -> Arc<Self> {
		let shared = Arc::new(Shared {
			pending: Mutex::new(Pending::default()),
			wake: Condvar::new(),
			images: RwLock::new(HashMap::new()),
			retired: Mutex::new(Vec::new()),
			changed: watch::channel(0).0,
		});
		let worker_shared = shared.clone();
		let worker = thread::Builder::new()
			.name("video-textures".to_owned())
			.spawn(move || run_worker(worker_shared))
			.ok();
		Arc::new(Self {
			shared,
			worker: Mutex::new(worker),
			previews: Mutex::new(HashMap::new()),
		})
	}

	pub fn image(&self, key: StreamKey) -> Option<Arc<RenderImage>> {
		self.shared.images.read().get(&key).cloned()
	}

	pub fn take_retired(&self) -> Vec<Arc<RenderImage>> {
		std::mem::take(&mut *self.shared.retired.lock())
	}

	pub fn frames(&self) -> watch::Receiver<u64> {
		self.shared.changed.subscribe()
	}

	pub fn start_preview(&self, key: StreamKey, source: PreviewSource) {
		let mut previews = self.previews.lock();
		if previews.contains_key(&key) {
			return;
		}
		let stop = Arc::new(AtomicBool::new(false));
		let shared = self.shared.clone();
		let thread_stop = stop.clone();
		let spawned = thread::Builder::new()
			.name("video-preview".to_owned())
			.spawn(move || run_preview(shared, key, source, thread_stop));
		if spawned.is_ok() {
			previews.insert(key, Preview { stop });
		}
	}

	pub fn stop_preview(&self, key: StreamKey) {
		let preview = self.previews.lock().remove(&key);
		if let Some(preview) = preview {
			preview.stop.store(true, Ordering::Relaxed);
		}
		self.shared.remove(key);
	}

	pub fn is_previewing(&self, key: StreamKey) -> bool {
		self.previews.lock().contains_key(&key)
	}
}

impl VideoSink for VideoTextures {
	fn frame(&self, stream: VideoStream, frame: Arc<VideoFrame>) {
		self.shared.submit((stream.user_id, ui_kind(stream.kind)), frame);
	}

	fn ended(&self, stream: VideoStream) {
		self.shared.end((stream.user_id, ui_kind(stream.kind)));
	}
}

impl Drop for VideoTextures {
	fn drop(&mut self) {
		for (_, preview) in self.previews.lock().drain() {
			preview.stop.store(true, Ordering::Relaxed);
		}
		self.shared.pending.lock().closed = true;
		self.shared.wake.notify_all();
		if let Some(worker) = self.worker.lock().take() {
			let _ = worker.join();
		}
	}
}

fn render_image(frame: &VideoFrame, max_side: u32) -> Option<Arc<RenderImage>> {
	let scaled = scaled_bgra(frame, max_side)?;
	let image = RgbaImage::from_raw(scaled.width, scaled.height, scaled.data)?;
	Some(Arc::new(RenderImage::new(smallvec![Frame::new(image)])))
}

pub fn thumbnail(target: &ScreenTarget) -> Option<Arc<RenderImage>> {
	let mut source = ScreenSource::open(target, THUMBNAIL_FPS).ok()?;
	let mut image = None;
	for _ in 0..THUMBNAIL_ATTEMPTS {
		let frame = source.next_frame().ok()?;
		image = render_image(&frame, THUMBNAIL_MAX_SIDE);
		if image.is_some() {
			break;
		}
	}
	image
}
