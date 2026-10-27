use std::time::Duration;

use crate::Frame;

const HD_HEIGHT: usize = 720;
const BGRA: usize = 4;

pub enum Chroma<'a> {
	Interleaved { uv: &'a [u8], stride: usize },
	Planar { u: &'a [u8], v: &'a [u8], stride: usize },
}

pub struct Picture<'a> {
	pub width: usize,
	pub height: usize,
	pub luma: &'a [u8],
	pub luma_stride: usize,
	pub chroma: Chroma<'a>,
	pub wide: bool,
	pub pts: Duration,
}

struct Matrix {
	luma: i32,
	red_v: i32,
	green_u: i32,
	green_v: i32,
	blue_u: i32,
}

const BT601: Matrix = Matrix {
	luma: 298,
	red_v: 409,
	green_u: 100,
	green_v: 208,
	blue_u: 516,
};

const BT709: Matrix = Matrix {
	luma: 298,
	red_v: 459,
	green_u: 55,
	green_v: 136,
	blue_u: 541,
};

fn channel(value: i32) -> u8 {
	(value >> 8).clamp(0, 255) as u8
}

pub fn step_for(width: usize, height: usize, max: (u32, u32)) -> usize {
	let (max_width, max_height) = (max.0.max(1) as usize, max.1.max(1) as usize);
	(width / max_width).min(height / max_height).max(1)
}

impl Picture<'_> {
	fn sample(&self, plane: &[u8], row: usize, column: usize) -> u32 {
		if self.wide {
			let at = row + column * 2;
			u32::from(u16::from_le_bytes([plane[at], plane[at + 1]]) >> 2)
		} else {
			u32::from(plane[row + column])
		}
	}

	fn chroma_at(&self, x: usize, y: usize) -> (i32, i32) {
		match self.chroma {
			Chroma::Interleaved { uv, stride } => {
				let row = y * stride;
				let column = x & !1;
				(
					self.sample(uv, row, column) as i32,
					self.sample(uv, row, column + 1) as i32,
				)
			}
			Chroma::Planar { u, v, stride } => {
				let row = y * stride;
				(
					self.sample(u, row, x / 2) as i32,
					self.sample(v, row, x / 2) as i32,
				)
			}
		}
	}

	fn luma_at(&self, x: usize, y: usize, step: usize) -> i32 {
		let row = y * self.luma_stride;
		if step == 1 {
			return self.sample(self.luma, row, x) as i32;
		}
		let below = row + self.luma_stride;
		let sum = self.sample(self.luma, row, x)
			+ self.sample(self.luma, row, x + 1)
			+ self.sample(self.luma, below, x)
			+ self.sample(self.luma, below, x + 1);
		(sum / 4) as i32
	}

	pub fn to_frame(&self, max: (u32, u32)) -> Frame {
		let step = step_for(self.width, self.height, max);
		let (out_width, out_height) = ((self.width / step).max(1), (self.height / step).max(1));
		let matrix = if self.height >= HD_HEIGHT { &BT709 } else { &BT601 };
		let mut bgra = vec![0u8; out_width * out_height * BGRA];
		for (row, line) in bgra.chunks_exact_mut(out_width * BGRA).enumerate() {
			let y = (row * step).min(self.height.saturating_sub(2));
			let chroma_row = y / 2;
			for (column, pixel) in line.as_chunks_mut::<BGRA>().0.iter_mut().enumerate() {
				let x = (column * step).min(self.width.saturating_sub(2));
				let c = (self.luma_at(x, y, step) - 16) * matrix.luma;
				let (u, v) = self.chroma_at(x, chroma_row);
				let (d, e) = (u - 128, v - 128);
				pixel[0] = channel(c + matrix.blue_u * d + 128);
				pixel[1] = channel(c - matrix.green_u * d - matrix.green_v * e + 128);
				pixel[2] = channel(c + matrix.red_v * e + 128);
				pixel[3] = u8::MAX;
			}
		}
		Frame {
			width: out_width as u32,
			height: out_height as u32,
			bgra,
			pts: self.pts,
		}
	}
}
