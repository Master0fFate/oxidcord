use gif::{DisposalMethod, Encoder, EncodingError, Frame, Repeat};
use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use plugin_sdk::model::FileBlob;
use plugin_sdk::types::OutgoingMessage;
use plugin_sdk::{Plugin, PluginExt, actions, data, export_plugin, log, ui};

const COMMAND: &str = "petpet";
const DESCRIPTION: &str = "Create a petpet gif: /petpet <user mention or id> [delay in ms]";
const USAGE: &str = "use /petpet <user mention or id> [delay in ms]";
const SIZE: u32 = 128;
const PRESS: [f32; 10] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 0.8, 0.6, 0.4, 0.2];
const DEFAULT_DELAY_MS: u32 = 20;
const MIN_DELAY_MS: u32 = 20;
const MAX_DELAY_MS: u32 = 1000;
const QUANTIZE_SPEED: i32 = 20;
const SKIN: Rgba<u8> = Rgba([255, 205, 148, 255]);
const OUTLINE: Rgba<u8> = Rgba([176, 118, 76, 255]);
const OUTLINE_WIDTH: f32 = 1.5;
const FINGER_CENTERS: [f32; 4] = [-0.14, -0.047, 0.047, 0.14];
const FINGER_HEIGHTS: [f32; 4] = [0.10, 0.13, 0.12, 0.09];
const FINGER_WIDTH: f32 = 0.085;

#[derive(Default)]
struct Petpet;

enum Shape {
	Ellipse {
		x: f32,
		y: f32,
		rx: f32,
		ry: f32,
	},
	Rect {
		left: f32,
		top: f32,
		right: f32,
		bottom: f32,
	},
}

impl Shape {
	fn bounds(&self, grow: f32) -> (f32, f32, f32, f32) {
		match *self {
			Self::Ellipse { x, y, rx, ry } => (x - rx - grow, y - ry - grow, x + rx + grow, y + ry + grow),
			Self::Rect {
				left,
				top,
				right,
				bottom,
			} => (left - grow, top - grow, right + grow, bottom + grow),
		}
	}

	fn contains(&self, px: f32, py: f32, grow: f32) -> bool {
		match *self {
			Self::Ellipse { x, y, rx, ry } => {
				let dx = (px - x) / (rx + grow);
				let dy = (py - y) / (ry + grow);
				dx * dx + dy * dy <= 1.0
			}
			Self::Rect { .. } => {
				let (left, top, right, bottom) = self.bounds(grow);
				(left..=right).contains(&px) && (top..=bottom).contains(&py)
			}
		}
	}

	fn paint(&self, canvas: &mut RgbaImage, grow: f32, color: Rgba<u8>) {
		let (left, top, right, bottom) = self.bounds(grow);
		let last = SIZE - 1;
		for y in (top.floor().max(0.0) as u32)..=(bottom.ceil().max(0.0) as u32).min(last) {
			for x in (left.floor().max(0.0) as u32)..=(right.ceil().max(0.0) as u32).min(last) {
				if self.contains(x as f32 + 0.5, y as f32 + 0.5, grow) {
					canvas.put_pixel(x, y, color);
				}
			}
		}
	}
}

fn hand(palm_y: f32) -> Vec<Shape> {
	let size = SIZE as f32;
	let middle = size * 0.5;
	let mut shapes = vec![
		Shape::Ellipse {
			x: middle,
			y: palm_y,
			rx: size * 0.21,
			ry: size * 0.075,
		},
		Shape::Ellipse {
			x: middle - size * 0.24,
			y: palm_y - size * 0.03,
			rx: size * 0.045,
			ry: size * 0.065,
		},
	];
	for (offset, height) in FINGER_CENTERS.iter().zip(FINGER_HEIGHTS) {
		let x = middle + offset * size;
		let half = size * FINGER_WIDTH / 2.0;
		let tip = palm_y - size * height;
		shapes.push(Shape::Rect {
			left: x - half,
			top: tip,
			right: x + half,
			bottom: palm_y,
		});
		shapes.push(Shape::Ellipse {
			x,
			y: tip,
			rx: half,
			ry: half,
		});
	}
	shapes
}

fn render_frame(avatar: &RgbaImage, press: f32) -> RgbaImage {
	let size = SIZE as f32;
	let width = (0.70 + 0.12 * press) * size;
	let height = (0.70 - 0.22 * press) * size;
	let top = 0.96 * size - height;
	let squashed = imageops::resize(
		avatar,
		width.round() as u32,
		height.round() as u32,
		FilterType::Triangle,
	);
	let mut canvas = RgbaImage::new(SIZE, SIZE);
	imageops::overlay(
		&mut canvas,
		&squashed,
		((size - width) / 2.0).round() as i64,
		top.round() as i64,
	);
	let shapes = hand(top);
	for shape in &shapes {
		shape.paint(&mut canvas, OUTLINE_WIDTH, OUTLINE);
	}
	for shape in &shapes {
		shape.paint(&mut canvas, 0.0, SKIN);
	}
	canvas
}

fn build_gif(avatar: &RgbaImage, delay: u16) -> Result<Vec<u8>, EncodingError> {
	let mut bytes = Vec::new();
	{
		let mut encoder = Encoder::new(&mut bytes, SIZE as u16, SIZE as u16, &[])?;
		encoder.set_repeat(Repeat::Infinite)?;
		for press in PRESS {
			let mut pixels = render_frame(avatar, press).into_raw();
			let mut frame = Frame::from_rgba_speed(SIZE as u16, SIZE as u16, &mut pixels, QUANTIZE_SPEED);
			frame.delay = delay;
			frame.dispose = DisposalMethod::Background;
			encoder.write_frame(&frame)?;
		}
	}
	Ok(bytes)
}

fn parse_user(token: &str) -> Option<u64> {
	let id = token
		.strip_prefix("<@")
		.and_then(|rest| rest.strip_suffix('>'))
		.map_or(token, |rest| rest.trim_start_matches('!'));
	id.parse().ok()
}

fn describe(error: impl std::fmt::Debug) -> String {
	format!("{error:?}")
}

impl Plugin for Petpet {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = ui::register_command(COMMAND, DESCRIPTION) {
			log::warn!("could not register /{COMMAND}: {error:?}");
		}
		Ok(())
	}

	fn on_command(&mut self, name: &str, args: &str, channel_id: u64) -> Result<(), String> {
		if name != COMMAND {
			return Ok(());
		}
		let mut parts = args.split_whitespace();
		let user_id = parts.next().and_then(parse_user).ok_or(USAGE)?;
		let delay_ms = match parts.next() {
			Some(raw) => raw.parse::<u32>().map_err(|_| USAGE)?,
			None => DEFAULT_DELAY_MS,
		};
		let delay = (delay_ms.clamp(MIN_DELAY_MS, MAX_DELAY_MS) / 10) as u16;
		let bytes = data::avatar_bytes(user_id, SIZE).map_err(describe)?;
		let avatar = image::load_from_memory(&bytes)
			.map_err(|error| format!("could not read the avatar: {error}"))?
			.to_rgba8();
		let gif = build_gif(&avatar, delay).map_err(|error| format!("could not encode the gif: {error}"))?;
		let message = OutgoingMessage {
			channel_id,
			content: String::new(),
			reply_to: None,
		};
		let file = FileBlob {
			name: "petpet.gif".to_owned(),
			content_type: Some("image/gif".to_owned()),
			bytes: gif,
		};
		actions::send_message_files(&message, &[file]).map_err(describe)
	}
}

impl PluginExt for Petpet {}

export_plugin!(Petpet, ext);
