use std::cell::Cell;
use std::rc::Rc;

use gpui::{
	Context, Entity, EventEmitter, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
	MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement as _, Pixels, Render, ScrollWheelEvent, Size,
	Styled as _, StyledImage as _, Subscription, Task, Window, canvas, div, img, prelude::FluentBuilder as _,
	px, size,
};
use protocol::Snowflake;
use state::State;

use super::video_sink::{follow_frames, video_image};
use crate::data::{Data, DataEvent};
use crate::theme::Theme;

const ZOOM_MAX: f32 = 8.;
const ZOOM_DOUBLE_CLICK: f32 = 2.5;
const ZOOM_PER_PIXEL: f32 = 0.0015;
const WHEEL_LINE: f32 = 16.;

pub enum ViewerEvent {
	Ended,
}

struct Drag {
	from: (f32, f32),
	pan: (f32, f32),
}

pub fn video_active(state: &State, user: Snowflake) -> bool {
	state.voice().is_some_and(|voice| {
		if state.current_user_id() == Some(user) {
			voice.camera_on || voice.screen_share.is_some()
		} else {
			voice.remote_video.contains_key(&user)
		}
	})
}

pub struct StreamViewer {
	user: Snowflake,
	zoom: f32,
	pan: (f32, f32),
	drag: Option<Drag>,
	area: Rc<Cell<Size<Pixels>>>,
	_frames: Task<()>,
	_ended: Subscription,
}

impl EventEmitter<ViewerEvent> for StreamViewer {}

impl StreamViewer {
	pub fn new(user: Snowflake, data: &Entity<Data>, cx: &mut Context<Self>) -> Self {
		let ended = cx.subscribe(data, |this, data, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.voice_session
				&& !video_active(&data.read(cx).state(), this.user)
			{
				cx.emit(ViewerEvent::Ended);
			}
		});
		Self {
			user,
			zoom: 1.,
			pan: (0., 0.),
			drag: None,
			area: Rc::new(Cell::new(size(px(0.), px(0.)))),
			_frames: follow_frames(cx, |_, cx| cx.notify()),
			_ended: ended,
		}
	}

	pub fn user(&self) -> Snowflake {
		self.user
	}

	fn limit_pan(&mut self) {
		let area = self.area.get();
		let reach = (
			(f32::from(area.width) * (self.zoom - 1.) / 2.).max(0.),
			(f32::from(area.height) * (self.zoom - 1.) / 2.).max(0.),
		);
		self.pan = (
			self.pan.0.clamp(-reach.0, reach.0),
			self.pan.1.clamp(-reach.1, reach.1),
		);
	}

	fn zoom_to(&mut self, zoom: f32, cx: &mut Context<Self>) {
		self.zoom = zoom.clamp(1., ZOOM_MAX);
		if self.zoom == 1. {
			self.pan = (0., 0.);
		}
		self.limit_pan();
		cx.notify();
	}

	fn on_wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
		let delta = f32::from(event.delta.pixel_delta(px(WHEEL_LINE)).y);
		self.zoom_to(self.zoom * (1. + delta * ZOOM_PER_PIXEL), cx);
	}

	fn on_press(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
		cx.stop_propagation();
		if event.click_count == 2 {
			self.zoom_to(if self.zoom == 1. { ZOOM_DOUBLE_CLICK } else { 1. }, cx);
			return;
		}
		self.drag = Some(Drag {
			from: (f32::from(event.position.x), f32::from(event.position.y)),
			pan: self.pan,
		});
	}

	fn on_drag(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
		let Some(drag) = &self.drag else {
			return;
		};
		if self.zoom == 1. {
			return;
		}
		self.pan = (
			drag.pan.0 + f32::from(event.position.x) - drag.from.0,
			drag.pan.1 + f32::from(event.position.y) - drag.from.1,
		);
		self.limit_pan();
		cx.notify();
	}

	fn on_release(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
		self.drag = None;
	}
}

impl Render for StreamViewer {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let measure = {
			let cell = self.area.clone();
			let entity = cx.entity();
			canvas(
				move |bounds, _, cx| {
					if cell.get() != bounds.size {
						cell.set(bounds.size);
						entity.update(cx, |_, cx| cx.notify());
					}
				},
				|_, _, _, _| {},
			)
			.absolute()
			.size_full()
		};
		let area = self.area.get();
		let (width, height) = (f32::from(area.width), f32::from(area.height));
		let layer = div().absolute();
		let layer = if self.zoom > 1. {
			layer
				.left(px(self.pan.0 - width * (self.zoom - 1.) / 2.))
				.top(px(self.pan.1 - height * (self.zoom - 1.) / 2.))
				.w(px(width * self.zoom))
				.h(px(height * self.zoom))
		} else {
			layer.size_full()
		};
		div()
			.relative()
			.size_full()
			.overflow_hidden()
			.bg(theme.background_floating)
			.on_scroll_wheel(cx.listener(Self::on_wheel))
			.on_mouse_down(MouseButton::Left, cx.listener(Self::on_press))
			.on_mouse_move(cx.listener(Self::on_drag))
			.on_mouse_up(MouseButton::Left, cx.listener(Self::on_release))
			.on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_release))
			.child(measure)
			.when_some(video_image(cx, self.user), |view, image| {
				view.child(layer.child(img(image).size_full().object_fit(ObjectFit::Contain)))
			})
	}
}
