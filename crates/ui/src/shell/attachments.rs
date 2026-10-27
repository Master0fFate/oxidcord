use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use bytes::Bytes;
use gpui::{
	AnyElement, App, AppContext as _, Bounds, Context, DragMoveEvent, Empty, Entity, EventEmitter,
	FontWeight, InteractiveElement as _, IntoElement, MouseButton, ObjectFit, ParentElement as _, Pixels,
	Point, Render, RenderImage, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
	Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat};
use media::{Decoded, Limits};
use rest::UploadFile;

use super::community::modal::{self, Close};
use super::composer::content_type;
use crate::components::{IconButton, dialog_footer, field};
use crate::format;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::images::{frame_id, render_image};
use crate::theme::{Theme, rem, with_alpha};

const SPOILER_PREFIX: &str = "SPOILER_";
const PREVIEW_SIDE: u32 = 1600;
const CARD_WIDTH: f32 = 200.;
const CARD_MEDIA: f32 = 150.;
const TOOLBAR_BUTTON: f32 = 32.;
const EDIT_MEDIA_WIDTH: f32 = 520.;
const EDIT_MEDIA_HEIGHT: f32 = 300.;
const VIEW_SHARE: f32 = 0.8;
const GRIP_SIZE: f32 = 12.;
const MIN_CROP: f32 = 0.05;
const MASK_ALPHA: f32 = 0.55;
const JPEG_QUALITY: u8 = 92;
const NAME_LIMIT: usize = 200;
const DESCRIPTION_LIMIT: usize = 1024;

#[derive(Clone)]
pub struct Preview {
	pub image: Arc<RenderImage>,
	pub width: u32,
	pub height: u32,
	pub croppable: bool,
}

pub struct Pending {
	pub id: u64,
	pub name: String,
	pub content_type: &'static str,
	pub bytes: Bytes,
	pub spoiler: bool,
	pub description: String,
	pub preview: Option<Preview>,
}

impl Pending {
	pub fn new(id: u64, name: String, bytes: Bytes) -> Self {
		Self {
			id,
			content_type: content_type(&name),
			name,
			bytes,
			spoiler: false,
			description: String::new(),
			preview: None,
		}
	}

	pub fn is_image(&self) -> bool {
		self.content_type.starts_with("image/")
	}

	pub fn upload(self) -> UploadFile {
		let name = if self.spoiler && !self.name.starts_with(SPOILER_PREFIX) {
			format!("{SPOILER_PREFIX}{}", self.name)
		} else {
			self.name
		};
		let description = self.description.trim();
		UploadFile {
			content_type: content_type(&name).to_owned(),
			description: (!description.is_empty()).then(|| description.to_owned()),
			name,
			bytes: self.bytes,
		}
	}
}

pub struct DecodedPreview {
	decoded: Decoded,
	croppable: bool,
}

pub fn decode_preview(bytes: &[u8]) -> Option<DecodedPreview> {
	let decoded = media::decode(bytes, &Limits::default())
		.inspect_err(|error| tracing::debug!(%error, "attachment preview skipped"))
		.ok()?;
	let croppable = matches!(decoded, Decoded::Still(_))
		&& image::guess_format(bytes).is_ok_and(|format| format != ImageFormat::Gif);
	Some(DecodedPreview {
		decoded: media::resize_decoded(decoded, PREVIEW_SIDE, PREVIEW_SIDE),
		croppable,
	})
}

pub fn preview(decoded: DecodedPreview) -> Option<Preview> {
	let (width, height) = decoded.decoded.dimensions();
	let (image, _) = render_image(&decoded.decoded, false)?;
	Some(Preview {
		image,
		width,
		height,
		croppable: decoded.croppable,
	})
}

#[derive(Clone, Copy, PartialEq)]
pub struct CropRect {
	left: f32,
	top: f32,
	right: f32,
	bottom: f32,
}

impl CropRect {
	const FULL: Self = Self {
		left: 0.,
		top: 0.,
		right: 1.,
		bottom: 1.,
	};

	fn width(&self) -> f32 {
		self.right - self.left
	}

	fn height(&self) -> f32 {
		self.bottom - self.top
	}
}

pub struct Cropped {
	pub bytes: Vec<u8>,
	pub name: String,
}

fn encoded(image: &DynamicImage, format: ImageFormat) -> Option<Vec<u8>> {
	let mut out = Cursor::new(Vec::new());
	let written = match format {
		ImageFormat::Jpeg => image
			.to_rgb8()
			.write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)),
		format => image.write_to(&mut out, format),
	};
	written
		.inspect_err(|error| tracing::warn!(%error, "could not encode the cropped image"))
		.ok()?;
	Some(out.into_inner())
}

pub fn crop(bytes: &[u8], name: &str, rect: CropRect) -> Option<Cropped> {
	let format = image::guess_format(bytes).ok()?;
	let source = image::load_from_memory_with_format(bytes, format)
		.inspect_err(|error| tracing::warn!(%error, "could not decode the image to crop"))
		.ok()?;
	let (width, height) = (source.width() as f32, source.height() as f32);
	let x = (rect.left * width).round() as u32;
	let y = (rect.top * height).round() as u32;
	let w = ((rect.width() * width).round() as u32).clamp(1, source.width() - x.min(source.width() - 1));
	let h = ((rect.height() * height).round() as u32).clamp(1, source.height() - y.min(source.height() - 1));
	let cut = source.crop_imm(x, y, w, h);
	let kept = matches!(
		format,
		ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Bmp
	);
	let target = if kept { format } else { ImageFormat::Png };
	let name = if kept {
		name.to_owned()
	} else {
		let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
		format!("{stem}.png")
	};
	Some(Cropped {
		bytes: encoded(&cut, target)?,
		name,
	})
}

#[derive(Clone, Copy)]
pub enum CardAction {
	Spoiler,
	Modify,
	Remove,
	View,
}

type CardHandler = Rc<dyn Fn(CardAction, &mut Window, &mut App)>;

fn toolbar_button(
	id: (&'static str, u64),
	icon: IconKind,
	label: SharedString,
	danger: bool,
	action: CardAction,
	on_action: &CardHandler,
	theme: &Theme,
) -> AnyElement {
	let on_action = on_action.clone();
	let button = IconButton::new(id, icon)
		.icon_size(rem(20.))
		.hit_size(rem(TOOLBAR_BUTTON))
		.tooltip(label)
		.on_click(move |_, window, cx| on_action(action, window, cx));
	if danger {
		button.color(theme.danger).into_any_element()
	} else {
		button.into_any_element()
	}
}

fn spoiler_cover(theme: &Theme, cx: &App) -> AnyElement {
	div()
		.absolute()
		.inset_0()
		.flex()
		.items_center()
		.justify_center()
		.bg(with_alpha(theme.background_tertiary, 0.92))
		.child(
			div()
				.px(theme.space_8)
				.py(theme.space_4)
				.rounded_full()
				.bg(with_alpha(theme.background_floating, 0.9))
				.text_size(theme.font_tiny)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_primary)
				.child(cx.t("attachment-spoiler-tag")),
		)
		.into_any_element()
}

fn card_media(pending: &Pending, on_action: &CardHandler, theme: &Theme, cx: &App) -> AnyElement {
	let frame = div()
		.id(("attachment-media", pending.id))
		.relative()
		.flex()
		.items_center()
		.justify_center()
		.w_full()
		.h(rem(CARD_MEDIA))
		.rounded(theme.radius_sm)
		.overflow_hidden()
		.bg(theme.background_tertiary);
	let Some(preview) = &pending.preview else {
		let icon = if pending.content_type.starts_with("video/") {
			IconKind::Video
		} else {
			IconKind::File
		};
		return frame
			.child(Icon::new(icon).size(rem(48.)).color(theme.interactive_normal))
			.when(pending.spoiler, |frame| frame.child(spoiler_cover(theme, cx)))
			.into_any_element();
	};
	let on_action = on_action.clone();
	frame
		.cursor_pointer()
		.on_click(move |_, window, cx| on_action(CardAction::View, window, cx))
		.child(
			img(preview.image.clone())
				.id(("attachment-preview", pending.id))
				.size_full()
				.object_fit(ObjectFit::Contain),
		)
		.when(pending.spoiler, |frame| frame.child(spoiler_cover(theme, cx)))
		.into_any_element()
}

pub fn card(
	pending: &Pending,
	on_action: impl Fn(CardAction, &mut Window, &mut App) + 'static,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	let on_action: CardHandler = Rc::new(on_action);
	let group = SharedString::from(format!("attachment-{}", pending.id));
	let id = pending.id;
	let toolbar = div()
		.absolute()
		.top(rem(-10.))
		.right(rem(-10.))
		.flex()
		.items_center()
		.gap(theme.space_4)
		.p(theme.space_4)
		.rounded(theme.radius_md)
		.border_1()
		.border_color(theme.divider)
		.bg(theme.background_floating)
		.occlude()
		.opacity(0.)
		.group_hover(group.clone(), |style| style.opacity(1.))
		.child(toolbar_button(
			("attachment-spoiler", id),
			if pending.spoiler {
				IconKind::EyeOff
			} else {
				IconKind::Eye
			},
			cx.t(if pending.spoiler {
				"attachment-unspoiler"
			} else {
				"attachment-spoiler"
			}),
			false,
			CardAction::Spoiler,
			&on_action,
			theme,
		))
		.child(toolbar_button(
			("attachment-modify", id),
			IconKind::Edit,
			cx.t("attachment-modify"),
			false,
			CardAction::Modify,
			&on_action,
			theme,
		))
		.child(toolbar_button(
			("attachment-remove", id),
			IconKind::Trash,
			cx.t("attachment-remove"),
			true,
			CardAction::Remove,
			&on_action,
			theme,
		));
	div()
		.id(("attachment-card", id))
		.group(group)
		.relative()
		.flex()
		.flex_col()
		.flex_none()
		.gap(theme.space_8)
		.w(rem(CARD_WIDTH))
		.p(theme.space_8)
		.rounded(theme.radius_md)
		.border_1()
		.border_color(theme.divider)
		.bg(theme.background_secondary)
		.child(card_media(pending, &on_action, theme, cx))
		.child(
			div()
				.flex()
				.flex_col()
				.min_w_0()
				.child(
					div()
						.truncate()
						.text_size(theme.font_sm)
						.text_color(theme.text_normal)
						.child(pending.name.clone()),
				)
				.child(
					div()
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.child(format::file_size(pending.bytes.len() as u64)),
				),
		)
		.child(toolbar)
		.into_any_element()
}

fn fitted(preview: &Preview, max_width: f32, max_height: f32) -> (f32, f32) {
	let (width, height) = (preview.width.max(1) as f32, preview.height.max(1) as f32);
	let scale = (max_width / width).min(max_height / height).min(1.);
	((width * scale).round(), (height * scale).round())
}

pub struct FullView {
	preview: Preview,
	width: f32,
	height: f32,
}

impl EventEmitter<Close> for FullView {}

pub fn open_full_view(window: &mut Window, cx: &mut App, name: String, preview: Preview) {
	let unit = rem(1.).to_pixels(window.rem_size());
	let viewport = window.viewport_size();
	let (width, height) = fitted(
		&preview,
		viewport.width / unit * VIEW_SHARE,
		viewport.height / unit * VIEW_SHARE - 120.,
	);
	let view = cx.new(|_| FullView {
		preview,
		width,
		height,
	});
	modal::open(window, cx, name, rem(width + 48.), &view, None);
}

impl Render for FullView {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div().flex().justify_center().child(
			img(self.preview.image.clone())
				.id(frame_id("attachment-full-view"))
				.w(rem(self.width))
				.h(rem(self.height))
				.object_fit(ObjectFit::Contain),
		)
	}
}

pub struct Edit {
	pub name: String,
	pub description: String,
	pub spoiler: bool,
	pub crop: Option<CropRect>,
}

type SaveHandler = Rc<dyn Fn(Edit, &mut App)>;

#[derive(Clone, Copy, PartialEq)]
enum Grip {
	Move,
	TopLeft,
	TopRight,
	BottomLeft,
	BottomRight,
}

impl Render for Grip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		Empty
	}
}

pub struct ModifyAttachment {
	name: Entity<InputState>,
	description: Entity<TextareaState>,
	spoiler: bool,
	preview: Option<Preview>,
	crop: CropRect,
	grab: Point<Pixels>,
	on_save: SaveHandler,
	_subscription: Subscription,
}

impl EventEmitter<Close> for ModifyAttachment {}

pub fn open_modify(
	window: &mut Window,
	cx: &mut App,
	pending: &Pending,
	on_save: impl Fn(Edit, &mut App) + 'static,
) {
	let name = pending.name.clone();
	let description = pending.description.clone();
	let spoiler = pending.spoiler;
	let preview = pending.preview.clone();
	let on_save: SaveHandler = Rc::new(on_save);
	let view = cx.new(|cx| {
		let name_input = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(name, window, cx);
			state
		});
		let placeholder = cx.t("attachment-description-placeholder");
		let description_input = cx.new(|cx| {
			let mut state = TextareaState::new(window, cx)
				.auto_grow(3, 6)
				.placeholder(placeholder);
			state.set_value(description, window, cx);
			state
		});
		let subscription = cx.subscribe(&name_input, |_, _, event: &InputEvent, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		ModifyAttachment {
			name: name_input,
			description: description_input,
			spoiler,
			preview,
			crop: CropRect::FULL,
			grab: Point::default(),
			on_save,
			_subscription: subscription,
		}
	});
	let title = cx.t("attachment-modify-title");
	let width = cx.global::<Theme>().modal_width_md;
	modal::open(window, cx, title, width, &view, None);
}

fn normalized(position: Point<Pixels>, bounds: Bounds<Pixels>) -> (f32, f32) {
	let x = (position.x - bounds.origin.x) / bounds.size.width;
	let y = (position.y - bounds.origin.y) / bounds.size.height;
	(x.clamp(0., 1.), y.clamp(0., 1.))
}

impl ModifyAttachment {
	fn trimmed_name(&self, cx: &App) -> String {
		self.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.filter(|character| !matches!(character, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
			.take(NAME_LIMIT)
			.collect()
	}

	fn save(&mut self, cx: &mut Context<Self>) {
		let name = self.trimmed_name(cx);
		if name.is_empty() {
			return;
		}
		let description = self
			.description
			.read(cx)
			.value()
			.chars()
			.take(DESCRIPTION_LIMIT)
			.collect();
		(self.on_save)(
			Edit {
				name,
				description,
				spoiler: self.spoiler,
				crop: (self.crop != CropRect::FULL).then_some(self.crop),
			},
			cx,
		);
		cx.emit(Close);
	}

	fn drag_crop(&mut self, event: &DragMoveEvent<Grip>, _: &mut Window, cx: &mut Context<Self>) {
		let bounds = event.bounds;
		let crop = &mut self.crop;
		match *event.drag(cx) {
			Grip::Move => {
				let origin = event.event.position - self.grab;
				let (x, y) = normalized(origin, bounds);
				let (width, height) = (crop.width(), crop.height());
				crop.left = x.min(1. - width);
				crop.top = y.min(1. - height);
				crop.right = crop.left + width;
				crop.bottom = crop.top + height;
			}
			grip => {
				let (x, y) = normalized(event.event.position, bounds);
				if matches!(grip, Grip::TopLeft | Grip::BottomLeft) {
					crop.left = x.min(crop.right - MIN_CROP);
				} else {
					crop.right = x.max(crop.left + MIN_CROP);
				}
				if matches!(grip, Grip::TopLeft | Grip::TopRight) {
					crop.top = y.min(crop.bottom - MIN_CROP);
				} else {
					crop.bottom = y.max(crop.top + MIN_CROP);
				}
			}
		}
		cx.notify();
	}

	fn grip(grip: Grip, id: &'static str) -> AnyElement {
		let half = rem(-GRIP_SIZE / 2.);
		let handle = div()
			.id(id)
			.absolute()
			.size(rem(GRIP_SIZE))
			.rounded(rem(2.))
			.border_1()
			.border_color(gpui::black())
			.bg(gpui::white())
			.cursor_crosshair()
			.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
			.on_drag(grip, |grip, _, _, cx| cx.new(|_| *grip));
		let handle = match grip {
			Grip::TopLeft => handle.top(half).left(half),
			Grip::TopRight => handle.top(half).right(half),
			Grip::BottomLeft => handle.bottom(half).left(half),
			_ => handle.bottom(half).right(half),
		};
		handle.into_any_element()
	}

	fn cropper(&self, preview: &Preview, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let (width, height) = fitted(preview, EDIT_MEDIA_WIDTH, EDIT_MEDIA_HEIGHT);
		let crop = self.crop;
		let mask = with_alpha(gpui::black(), MASK_ALPHA);
		let shade = |left: f32, top: f32, w: f32, h: f32| {
			div()
				.absolute()
				.left(rem(left * width))
				.top(rem(top * height))
				.w(rem(w * width))
				.h(rem(h * height))
				.bg(mask)
		};
		let weak = cx.weak_entity();
		let frame = div()
			.id("crop-frame")
			.absolute()
			.left(rem(crop.left * width))
			.top(rem(crop.top * height))
			.w(rem(crop.width() * width))
			.h(rem(crop.height() * height))
			.border_2()
			.border_color(gpui::white())
			.cursor_move()
			.on_drag(Grip::Move, move |grip, offset, _, cx| {
				weak.update(cx, |this, _| this.grab = offset).ok();
				cx.new(|_| *grip)
			})
			.child(Self::grip(Grip::TopLeft, "crop-top-left"))
			.child(Self::grip(Grip::TopRight, "crop-top-right"))
			.child(Self::grip(Grip::BottomLeft, "crop-bottom-left"))
			.child(Self::grip(Grip::BottomRight, "crop-bottom-right"));
		div()
			.flex()
			.flex_col()
			.items_center()
			.gap(theme.space_8)
			.child(
				div()
					.id("crop-area")
					.relative()
					.w(rem(width))
					.h(rem(height))
					.on_drag_move(cx.listener(Self::drag_crop))
					.child(
						img(preview.image.clone())
							.id(frame_id("attachment-crop"))
							.size_full(),
					)
					.child(shade(0., 0., 1., crop.top))
					.child(shade(0., crop.bottom, 1., 1. - crop.bottom))
					.child(shade(0., crop.top, crop.left, crop.height()))
					.child(shade(crop.right, crop.top, 1. - crop.right, crop.height()))
					.child(frame),
			)
			.when(crop != CropRect::FULL, |column| {
				column.child(
					div()
						.id("crop-reset")
						.text_size(theme.font_sm)
						.text_color(theme.link)
						.cursor_pointer()
						.hover(|style| style.underline())
						.on_click(cx.listener(|this, _, _, cx| {
							this.crop = CropRect::FULL;
							cx.notify();
						}))
						.child(cx.t("attachment-crop-reset")),
				)
			})
			.into_any_element()
	}

	fn still(&self, preview: &Preview, theme: &Theme) -> AnyElement {
		let (width, height) = fitted(preview, EDIT_MEDIA_WIDTH, EDIT_MEDIA_HEIGHT);
		div()
			.flex()
			.justify_center()
			.child(
				div()
					.w(rem(width))
					.h(rem(height))
					.rounded(theme.radius_sm)
					.overflow_hidden()
					.child(
						img(preview.image.clone())
							.id(frame_id("attachment-edit-preview"))
							.size_full(),
					),
			)
			.into_any_element()
	}
}

impl Render for ModifyAttachment {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let media = self.preview.clone().map(|preview| {
			if preview.croppable {
				self.cropper(&preview, &theme, cx)
			} else {
				self.still(&preview, &theme)
			}
		});
		let enabled = !self.trimmed_name(cx).is_empty();
		let weak_cancel = cx.weak_entity();
		let weak_save = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.children(media)
			.child(field(cx.t("attachment-filename"), &theme, Input::new(&self.name)))
			.child(field(
				cx.t("attachment-description"),
				&theme,
				div()
					.rounded(theme.radius_md)
					.bg(theme.input_background)
					.px(theme.space_12)
					.py(theme.space_8)
					.child(
						Textarea::new(&self.description)
							.appearance(false)
							.bordered(false)
							.p_0(),
					),
			))
			.child(
				Checkbox::new("attachment-spoiler-check")
					.checked(self.spoiler)
					.label(cx.t("attachment-mark-spoiler"))
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						this.spoiler = *checked;
						cx.notify();
					})),
			)
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("settings-save"),
				false,
				enabled,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_save.update(cx, |this, cx| this.save(cx)).ok();
				},
			))
	}
}
