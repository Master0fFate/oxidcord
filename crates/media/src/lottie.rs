use velato::model::{ImageAsset, fixed};
use velato::{Composition, RenderSink, Renderer};
use vello_cpu::kurbo::{Affine, Shape};
use vello_cpu::peniko::{BlendMode, Brush, Fill};
use vello_cpu::{PaintType, Pixmap, RenderContext, Resources};

use crate::cache::SizeHint;
use crate::decode::Limits;
use crate::error::{MediaError, Result};
use crate::types::{Animation, Decoded, DecodedImage, Frame};

const DEFAULT_SIDE: u32 = 320;
const MAX_SIDE: u32 = 1024;
const MAX_FPS: f64 = 24.;
const TOLERANCE: f64 = 0.1;

pub fn is_lottie(bytes: &[u8]) -> bool {
	bytes
		.iter()
		.find(|byte| !byte.is_ascii_whitespace())
		.is_some_and(|byte| *byte == b'{')
}

pub fn decode(bytes: &[u8], limits: &Limits, hint: SizeHint) -> Result<Decoded> {
	let composition =
		Composition::from_slice(bytes).map_err(|error| MediaError::Decode(error.to_string()))?;
	let (width, height) = output_size(&composition, hint, limits)?;
	let scale = f64::from(width) / composition.width.max(1) as f64;
	let transform = Affine::scale(scale);
	let plan = sample_plan(&composition, width, height, limits);
	let mut context = RenderContext::new(width as u16, height as u16);
	let mut resources = Resources::new();
	let mut renderer = Renderer::new();
	let mut pixmap = Pixmap::new(width as u16, height as u16);
	let mut frames = Vec::with_capacity(plan.count);
	for index in 0..plan.count {
		let frame = composition.frames.start + index as f64 * plan.step;
		context.reset();
		context.set_fill_rule(Fill::NonZero);
		renderer.append(&composition, frame, transform, 1., &mut Sink(&mut context));
		context.flush();
		context.render(&mut pixmap, &mut resources);
		frames.push(Frame {
			image: straight_rgba(&pixmap, width, height),
			delay_ms: plan.delay_ms,
		});
	}
	match frames.len() {
		0 => Err(MediaError::Decode("animation has no frames".to_owned())),
		1 => Ok(Decoded::Still(frames.remove(0).image)),
		_ => Ok(Decoded::Animated(Animation { frames, loops: None })),
	}
}

struct SamplePlan {
	count: usize,
	step: f64,
	delay_ms: u32,
}

fn sample_plan(composition: &Composition, width: u32, height: u32, limits: &Limits) -> SamplePlan {
	let total_frames = (composition.frames.end - composition.frames.start).max(0.);
	let frame_rate = if composition.frame_rate.is_finite() && composition.frame_rate > 0. {
		composition.frame_rate
	} else {
		MAX_FPS
	};
	let duration = total_frames / frame_rate;
	if limits.first_frame_only || duration <= 0. {
		return SamplePlan {
			count: 1,
			step: 0.,
			delay_ms: 0,
		};
	}
	let frame_bytes = u64::from(width) * u64::from(height) * 4;
	let by_memory = (limits.max_animation_bytes as u64 / frame_bytes.max(1)).max(1);
	let wanted = (duration * frame_rate.min(MAX_FPS)).ceil() as u64;
	let count = wanted
		.clamp(1, u64::from(limits.max_frames).max(1))
		.min(by_memory) as usize;
	SamplePlan {
		count,
		step: total_frames / count as f64,
		delay_ms: (duration * 1000. / count as f64).round().max(1.) as u32,
	}
}

fn output_size(composition: &Composition, hint: SizeHint, limits: &Limits) -> Result<(u32, u32)> {
	let source_width = composition.width.max(1) as f64;
	let source_height = composition.height.max(1) as f64;
	let (bound_width, bound_height) = if hint.is_original() {
		(DEFAULT_SIDE, DEFAULT_SIDE)
	} else {
		(hint.width.min(MAX_SIDE), hint.height.min(MAX_SIDE))
	};
	let scale = (f64::from(bound_width) / source_width).min(f64::from(bound_height) / source_height);
	let width = ((source_width * scale).round() as u32).max(1);
	let height = ((source_height * scale).round() as u32).max(1);
	let pixels = u64::from(width) * u64::from(height);
	if pixels > limits.max_pixels {
		return Err(MediaError::Limit(format!(
			"{width}x{height} exceeds {} pixels",
			limits.max_pixels
		)));
	}
	Ok((width, height))
}

fn straight_rgba(pixmap: &Pixmap, width: u32, height: u32) -> DecodedImage {
	let mut rgba = pixmap.data_as_u8_slice().to_vec();
	for pixel in rgba.as_chunks_mut::<4>().0 {
		let alpha = pixel[3];
		if alpha != 0 && alpha != 255 {
			for channel in &mut pixel[..3] {
				*channel =
					((u16::from(*channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255) as u8;
			}
		}
	}
	DecodedImage { width, height, rgba }
}

struct Sink<'a>(&'a mut RenderContext);

impl RenderSink for Sink<'_> {
	fn push_layer(&mut self, blend: impl Into<BlendMode>, alpha: f32, transform: Affine, shape: &impl Shape) {
		self.0.set_transform(transform);
		self.0.push_layer(
			Some(&shape.to_path(TOLERANCE)),
			Some(blend.into()),
			Some(alpha),
			None,
			None,
		);
	}

	fn push_clip_layer(&mut self, transform: Affine, shape: &impl Shape) {
		self.0.set_transform(transform);
		self.0.push_clip_layer(&shape.to_path(TOLERANCE));
	}

	fn pop_layer(&mut self) {
		self.0.pop_layer();
	}

	fn draw(
		&mut self,
		stroke: Option<&fixed::Stroke>,
		transform: Affine,
		brush: &fixed::Brush,
		shape: &impl Shape,
	) {
		let paint = match brush {
			Brush::Solid(color) => PaintType::Solid(*color),
			Brush::Gradient(gradient) => PaintType::Gradient(gradient.clone()),
			Brush::Image(_) => return,
		};
		self.0.set_transform(transform);
		self.0.set_paint(paint);
		let path = shape.to_path(TOLERANCE);
		match stroke {
			Some(stroke) => {
				self.0.set_stroke(stroke.clone());
				self.0.stroke_path(&path);
			}
			None => self.0.fill_path(&path),
		}
	}

	fn draw_image(&mut self, _: &ImageAsset, _: Affine, _: f64) {}
}
