use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
	App, Bounds, Corners, EntityId, Global, IntoElement, Pixels, RenderImage, Styled as _, Window, canvas,
	point, size,
};
use image::{Frame, RgbaImage};
use media::MediaService;
use player::{Options, Player, State, Status};
use smallvec::SmallVec;
use tokio::sync::Notify;
use url::Url;

use crate::images::mock_url;
use crate::webframe::{self, Host, Rect};

const HIDDEN_GRACE: Duration = Duration::from_secs(1);
const LOOP_GRACE: Duration = Duration::from_millis(1500);
const MEDIA_GRACE: Duration = Duration::from_secs(10);
const SWEEP: Duration = Duration::from_millis(500);
const FRAME_SWEEP: Duration = Duration::from_millis(100);
const SIZE_HINT: f32 = 2.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaKey {
	pub message: u64,
	pub item: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
	Media,
	Loop,
}

struct Slot {
	player: Player,
	mode: Mode,
	image: Option<Arc<RenderImage>>,
	status: Status,
	view: Option<EntityId>,
	epoch: u64,
	seen_at: Instant,
	bar: Option<Bounds<Pixels>>,
	scrub: Option<f32>,
}

struct FrameSlot {
	id: u64,
	view: Option<EntityId>,
	epoch: u64,
	placed: Option<(Rect, Rect)>,
}

pub struct Playback {
	media: MediaService,
	mock_origin: Option<Url>,
	wake: Arc<Notify>,
	slots: RefCell<HashMap<MediaKey, Slot>>,
	volume: Cell<f32>,
	muted: Cell<bool>,
	volume_bar: Cell<Option<Bounds<Pixels>>>,
	epochs: RefCell<HashMap<EntityId, u64>>,
	frames: RefCell<HashMap<MediaKey, FrameSlot>>,
	host: RefCell<Option<Host>>,
	next_frame: Cell<u64>,
	frame_data: PathBuf,
	shell_hidden: Cell<bool>,
	dialog_open: Cell<bool>,
}

impl Global for Playback {}

pub fn hint_size(width: f32, height: f32) -> (u32, u32) {
	((width * SIZE_HINT) as u32, (height * SIZE_HINT) as u32)
}

fn render(frame: player::Frame) -> Option<Arc<RenderImage>> {
	let buffer = RgbaImage::from_raw(frame.width, frame.height, frame.bgra)?;
	Some(Arc::new(RenderImage::new(smallvec::smallvec![Frame::new(
		buffer
	)])))
}

fn contained(bounds: Bounds<Pixels>, width: u32, height: u32) -> Bounds<Pixels> {
	let (box_width, box_height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
	let scale = (box_width / width.max(1) as f32).min(box_height / height.max(1) as f32);
	let (shown_width, shown_height) = (width as f32 * scale, height as f32 * scale);
	Bounds::new(
		point(
			bounds.origin.x + Pixels::from((box_width - shown_width) / 2.),
			bounds.origin.y + Pixels::from((box_height - shown_height) / 2.),
		),
		size(Pixels::from(shown_width), Pixels::from(shown_height)),
	)
}

fn fraction(bar: Bounds<Pixels>, x: Pixels) -> f32 {
	let width = f32::from(bar.size.width);
	if width <= 0. {
		return 0.;
	}
	(f32::from(x - bar.origin.x) / width).clamp(0., 1.)
}

impl Playback {
	pub fn install(media: MediaService, mock_origin: Option<Url>, frame_data: PathBuf, cx: &mut App) {
		let wake = Arc::new(Notify::new());
		cx.set_global(Self {
			media,
			mock_origin,
			wake: wake.clone(),
			slots: RefCell::new(HashMap::new()),
			volume: Cell::new(1.),
			muted: Cell::new(false),
			volume_bar: Cell::new(None),
			epochs: RefCell::new(HashMap::new()),
			frames: RefCell::new(HashMap::new()),
			host: RefCell::new(None),
			next_frame: Cell::new(0),
			frame_data,
			shell_hidden: Cell::new(false),
			dialog_open: Cell::new(false),
		});
		cx.spawn(async move |cx| {
			loop {
				let sweep = cx.update(|cx| cx.global::<Self>().sweep_interval());
				if let Some(sweep) = sweep {
					let sweep = cx.background_executor().timer(sweep);
					tokio::select! {
						() = wake.notified() => {}
						() = sweep => {}
					}
				} else {
					wake.notified().await;
				}
				cx.update(Self::pump);
			}
		})
		.detach();
	}

	fn sweep_interval(&self) -> Option<Duration> {
		if !self.frames.borrow().is_empty() {
			Some(FRAME_SWEEP)
		} else if !self.slots.borrow().is_empty() {
			Some(SWEEP)
		} else {
			None
		}
	}

	pub fn view_marker() -> impl IntoElement {
		canvas(
			|_, _, _| {},
			|_, _, window, cx| cx.global::<Self>().mark_view(window.current_view()),
		)
		.size_0()
	}

	fn mark_view(&self, view: EntityId) {
		let painted = self.epoch(view);
		let host = self.host.borrow();
		self.frames.borrow_mut().retain(|_, slot| {
			let keep = slot.view != Some(view) || slot.epoch == painted;
			if !keep && let Some(host) = host.as_ref() {
				host.close(slot.id);
			}
			keep
		});
		*self.epochs.borrow_mut().entry(view).or_default() += 1;
	}

	fn hidden(&self) -> bool {
		self.shell_hidden.get() || self.dialog_open.get()
	}

	fn set_hidden(&self, shell: Option<bool>, dialog: Option<bool>) {
		let before = self.hidden();
		if let Some(shell) = shell {
			self.shell_hidden.set(shell);
		}
		if let Some(dialog) = dialog {
			self.dialog_open.set(dialog);
		}
		let hidden = self.hidden();
		if hidden == before {
			return;
		}
		let host = self.host.borrow();
		let Some(host) = host.as_ref() else {
			return;
		};
		for slot in self.frames.borrow().values() {
			match (hidden, slot.placed) {
				(true, _) => host.hide(slot.id),
				(false, Some((rect, clip))) => host.place(slot.id, rect, clip),
				(false, None) => {}
			}
		}
	}

	pub fn set_shell_hidden(&self, hidden: bool) {
		self.set_hidden(Some(hidden), None);
	}

	pub fn frame_active(&self, key: MediaKey) -> bool {
		self.frames.borrow().contains_key(&key)
	}

	pub fn activate_frame(&self, key: MediaKey, document: String, window: &Window) -> bool {
		self.pause_others(key);
		if self.frames.borrow().contains_key(&key) {
			return true;
		}
		let mut host = self.host.borrow_mut();
		if host.is_none() {
			*host = webframe::owner(window).and_then(|owner| Host::start(owner, self.frame_data.clone()));
		}
		let Some(host) = host.as_ref() else {
			return false;
		};
		let id = self.next_frame.get() + 1;
		self.next_frame.set(id);
		host.open(id, document);
		self.frames.borrow_mut().insert(
			key,
			FrameSlot {
				id,
				view: None,
				epoch: 0,
				placed: None,
			},
		);
		self.wake.notify_one();
		true
	}

	pub fn paint_frame(&self, key: MediaKey, bounds: Bounds<Pixels>, window: &Window) {
		let view = window.current_view();
		let epoch = self.epoch(view);
		let scale = window.scale_factor();
		let physical = |area: Bounds<Pixels>| Rect {
			x: (f32::from(area.origin.x) * scale).round() as i32,
			y: (f32::from(area.origin.y) * scale).round() as i32,
			width: (f32::from(area.size.width) * scale).round() as i32,
			height: (f32::from(area.size.height) * scale).round() as i32,
		};
		let rect = physical(bounds);
		let clip = physical(bounds.intersect(&window.content_mask().bounds));
		let mut frames = self.frames.borrow_mut();
		let Some(slot) = frames.get_mut(&key) else {
			return;
		};
		slot.view = Some(view);
		slot.epoch = epoch;
		if slot.placed == Some((rect, clip)) {
			return;
		}
		slot.placed = Some((rect, clip));
		if !self.hidden()
			&& let Some(host) = self.host.borrow().as_ref()
		{
			host.place(slot.id, rect, clip);
		}
	}

	fn epoch(&self, view: EntityId) -> u64 {
		self.epochs.borrow().get(&view).copied().unwrap_or_default()
	}

	fn resolve(&self, url: &str) -> String {
		match &self.mock_origin {
			Some(origin) => mock_url(origin, url),
			None => url.to_owned(),
		}
	}

	fn effective_volume(&self) -> f32 {
		if self.muted.get() { 0. } else { self.volume.get() }
	}

	fn open(&self, key: MediaKey, url: &str, mode: Mode, max_size: (u32, u32)) {
		let wake = self.wake.clone();
		let options = Options {
			autoplay: true,
			looping: mode == Mode::Loop,
			volume: if mode == Mode::Loop {
				0.
			} else {
				self.effective_volume()
			},
			max_size,
		};
		let player = Player::open(
			self.media.fetcher().clone(),
			self.media.runtime().clone(),
			self.resolve(url),
			options,
			Arc::new(move || wake.notify_one()),
		);
		let status = player.status();
		self.slots.borrow_mut().insert(
			key,
			Slot {
				player,
				mode,
				image: None,
				status,
				view: None,
				epoch: 0,
				seen_at: Instant::now(),
				bar: None,
				scrub: None,
			},
		);
		self.wake.notify_one();
	}

	fn pause_others(&self, key: MediaKey) {
		for (other, slot) in self.slots.borrow().iter() {
			if *other != key && slot.mode == Mode::Media && slot.status.state == State::Playing {
				slot.player.pause();
			}
		}
	}

	pub fn toggle(&self, key: MediaKey, url: &str, max_size: (u32, u32)) {
		self.pause_others(key);
		let state = self.slots.borrow().get(&key).map(|slot| slot.status.state);
		match state {
			None | Some(State::Failed) => self.open(key, url, Mode::Media, max_size),
			Some(State::Playing) => self.with(key, |slot| slot.player.pause()),
			Some(_) => self.with(key, |slot| slot.player.play()),
		}
	}

	fn ensure_loop(&self, key: MediaKey, url: &str, max_size: (u32, u32)) {
		if !self.slots.borrow().contains_key(&key) {
			self.open(key, url, Mode::Loop, max_size);
		}
	}

	fn with(&self, key: MediaKey, action: impl FnOnce(&mut Slot)) {
		if let Some(slot) = self.slots.borrow_mut().get_mut(&key) {
			action(slot);
		}
	}

	pub fn status(&self, key: MediaKey) -> Option<Status> {
		self.slots.borrow().get(&key).map(|slot| slot.status)
	}

	pub fn has_frame(&self, key: MediaKey) -> bool {
		self.slots
			.borrow()
			.get(&key)
			.is_some_and(|slot| slot.image.is_some())
	}

	pub fn scrub(&self, key: MediaKey) -> Option<f32> {
		self.slots.borrow().get(&key).and_then(|slot| slot.scrub)
	}

	pub fn volume(&self) -> (f32, bool) {
		(self.volume.get(), self.muted.get())
	}

	fn apply_volume(&self) {
		let volume = self.effective_volume();
		for slot in self.slots.borrow().values() {
			if slot.mode == Mode::Media {
				slot.player.set_volume(volume);
			}
		}
	}

	pub fn toggle_mute(&self) {
		if self.volume.get() <= 0. {
			self.volume.set(1.);
			self.muted.set(false);
		} else {
			self.muted.set(!self.muted.get());
		}
		self.apply_volume();
	}

	pub fn set_volume_bar(&self, bounds: Bounds<Pixels>) {
		self.volume_bar.set(Some(bounds));
	}

	pub fn volume_at(&self, x: Pixels) {
		let Some(bar) = self.volume_bar.get() else {
			return;
		};
		self.volume.set(fraction(bar, x));
		self.muted.set(false);
		self.apply_volume();
	}

	pub fn set_bar(&self, key: MediaKey, bounds: Bounds<Pixels>) {
		self.with(key, |slot| slot.bar = Some(bounds));
	}

	pub fn scrub_at(&self, key: MediaKey, x: Pixels) {
		self.with(key, |slot| {
			if let Some(bar) = slot.bar {
				slot.scrub = Some(fraction(bar, x));
			}
		});
	}

	pub fn stop_all(&self) {
		self.slots.borrow_mut().clear();
		let mut frames = self.frames.borrow_mut();
		if let Some(host) = self.host.borrow().as_ref() {
			for slot in frames.values() {
				host.close(slot.id);
			}
		}
		frames.clear();
	}

	pub fn release(&self, key: MediaKey) {
		self.with(key, |slot| {
			let (Some(scrub), Some(duration)) = (slot.scrub.take(), slot.status.duration) else {
				return;
			};
			slot.player.seek(duration.mul_f32(scrub));
		});
	}

	pub fn paint_loop(
		&self,
		key: MediaKey,
		url: &str,
		bounds: Bounds<Pixels>,
		radius: Pixels,
		window: &mut Window,
	) {
		let size = bounds.size.map(f32::from);
		self.ensure_loop(key, url, hint_size(size.width, size.height));
		self.paint(key, bounds, radius, window);
	}

	pub fn paint(&self, key: MediaKey, bounds: Bounds<Pixels>, radius: Pixels, window: &mut Window) {
		let image = {
			let mut slots = self.slots.borrow_mut();
			let Some(slot) = slots.get_mut(&key) else {
				return;
			};
			let view = window.current_view();
			slot.view = Some(view);
			slot.epoch = self.epoch(view);
			slot.seen_at = Instant::now();
			slot.image.clone()
		};
		let Some(image) = image else {
			return;
		};
		let frame = image.size(0);
		let target = contained(bounds, frame.width.0 as u32, frame.height.0 as u32);
		if let Err(error) = window.paint_image(target, bounds, Corners::all(radius), image, 0, false) {
			tracing::debug!(%error, "could not paint a video frame");
		}
	}

	pub fn touch(&self, key: MediaKey, window: &Window) {
		let view = window.current_view();
		let epoch = self.epoch(view);
		self.with(key, |slot| {
			slot.view = Some(view);
			slot.epoch = epoch;
			slot.seen_at = Instant::now();
		});
	}

	fn pump(cx: &mut App) {
		if !cx.global::<Self>().frames.borrow().is_empty() {
			let dialog = cx.windows().into_iter().any(|handle| {
				handle
					.update(cx, |_, window, cx| {
						gpui_component::WindowExt::has_active_dialog(window, cx)
					})
					.unwrap_or(false)
			});
			cx.global::<Self>().set_hidden(None, Some(dialog));
		}
		let (retired, views) = cx.global::<Self>().collect();
		for image in retired {
			cx.drop_image(image, None);
		}
		for view in views {
			cx.notify(view);
		}
	}

	fn stale(slot: &Slot, epochs: &HashMap<EntityId, u64>) -> bool {
		let unseen = slot.seen_at.elapsed();
		let drawn_without = slot
			.view
			.and_then(|view| epochs.get(&view))
			.is_some_and(|epoch| *epoch > slot.epoch + 1);
		let grace = match slot.mode {
			Mode::Loop => LOOP_GRACE,
			Mode::Media => MEDIA_GRACE,
		};
		(drawn_without && unseen > HIDDEN_GRACE) || (slot.status.state == State::Playing && unseen > grace)
	}

	fn collect(&self) -> (Vec<Arc<RenderImage>>, SmallVec<[EntityId; 4]>) {
		let mut retired = Vec::new();
		let mut views: SmallVec<[EntityId; 4]> = SmallVec::new();
		let epochs = self.epochs.borrow();
		self.slots.borrow_mut().retain(|_, slot| {
			if Self::stale(slot, &epochs) {
				retired.extend(slot.image.take());
				return false;
			}
			let mut changed = false;
			if let Some(image) = slot.player.take_frame().and_then(render) {
				retired.extend(slot.image.replace(image));
				changed = true;
			}
			let status = slot.player.status();
			if status != slot.status {
				slot.status = status;
				changed = true;
			}
			if changed
				&& let Some(view) = slot.view
				&& !views.contains(&view)
			{
				views.push(view);
			}
			true
		});
		(retired, views)
	}
}
