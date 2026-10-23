use super::{PixelFormat, VideoError, VideoFrame};

const BGRA_BYTES: usize = 4;

pub struct Nv12Planes<'a> {
	pub luma: &'a [u8],
	pub chroma: &'a [u8],
}

pub fn split_nv12(data: &[u8], width: usize, height: usize) -> Nv12Planes<'_> {
	let (luma, chroma) = data.split_at(width * height);
	Nv12Planes { luma, chroma }
}

fn column_map(source: usize, target: usize) -> Vec<usize> {
	(0..target).map(|column| column * source / target).collect()
}

pub fn bgra_to_nv12(frame: &VideoFrame, width: usize, height: usize, out: &mut Vec<u8>) {
	let source_width = frame.width as usize;
	let source_height = frame.height as usize;
	let columns = column_map(source_width, width);
	out.clear();
	out.resize(width * height * 3 / 2, 0);
	let (luma, chroma) = out.split_at_mut(width * height);
	for row in 0..height {
		let source_row = row * source_height / height;
		let line = &frame.data[source_row * source_width * BGRA_BYTES..][..source_width * BGRA_BYTES];
		let target = &mut luma[row * width..][..width];
		for (pixel, column) in target.iter_mut().zip(&columns) {
			let offset = column * BGRA_BYTES;
			*pixel = luma_of(line[offset + 2], line[offset + 1], line[offset]);
		}
	}
	for row in 0..height / 2 {
		let upper = &frame.data[(row * 2 * source_height / height) * source_width * BGRA_BYTES..]
			[..source_width * BGRA_BYTES];
		let lower_row = ((row * 2 + 1) * source_height / height).min(source_height - 1);
		let lower = &frame.data[lower_row * source_width * BGRA_BYTES..][..source_width * BGRA_BYTES];
		let target = &mut chroma[row * width..][..width];
		for pair in 0..width / 2 {
			let left = columns[pair * 2] * BGRA_BYTES;
			let right = columns[pair * 2 + 1] * BGRA_BYTES;
			let mut blue = 0u32;
			let mut green = 0u32;
			let mut red = 0u32;
			for (line, offset) in [(upper, left), (upper, right), (lower, left), (lower, right)] {
				blue += u32::from(line[offset]);
				green += u32::from(line[offset + 1]);
				red += u32::from(line[offset + 2]);
			}
			let (u, v) = chroma_of((red / 4) as i32, (green / 4) as i32, (blue / 4) as i32);
			target[pair * 2] = u;
			target[pair * 2 + 1] = v;
		}
	}
}

fn luma_of(red: u8, green: u8, blue: u8) -> u8 {
	let value = 66 * i32::from(red) + 129 * i32::from(green) + 25 * i32::from(blue);
	((value + 128) >> 8) as u8 + 16
}

fn chroma_of(red: i32, green: i32, blue: i32) -> (u8, u8) {
	let u = (-38 * red - 74 * green + 112 * blue + 128) >> 8;
	let v = (112 * red - 94 * green - 18 * blue + 128) >> 8;
	((u + 128).clamp(0, 255) as u8, (v + 128).clamp(0, 255) as u8)
}

pub fn scale_nv12(frame: &VideoFrame, width: usize, height: usize, out: &mut Vec<u8>) {
	let source_width = frame.width as usize;
	let source_height = frame.height as usize;
	let source = split_nv12(&frame.data, source_width, source_height);
	let columns = column_map(source_width, width);
	let chroma_columns = column_map(source_width / 2, width / 2);
	out.clear();
	out.resize(width * height * 3 / 2, 0);
	let (luma, chroma) = out.split_at_mut(width * height);
	for row in 0..height {
		let line = &source.luma[(row * source_height / height) * source_width..][..source_width];
		for (pixel, column) in luma[row * width..][..width].iter_mut().zip(&columns) {
			*pixel = line[*column];
		}
	}
	for row in 0..height / 2 {
		let line = &source.chroma[(row * source_height / height) * source_width..][..source_width];
		let target = &mut chroma[row * width..][..width];
		for (pair, column) in chroma_columns.iter().enumerate() {
			target[pair * 2] = line[column * 2];
			target[pair * 2 + 1] = line[column * 2 + 1];
		}
	}
}

pub fn to_nv12(frame: &VideoFrame, width: usize, height: usize, out: &mut Vec<u8>) -> Result<(), VideoError> {
	if frame.data.len() != frame.expected_len() || frame.width < 2 || frame.height < 2 {
		return Err(VideoError::InvalidFrame("buffer does not match dimensions"));
	}
	match frame.format {
		PixelFormat::Bgra => bgra_to_nv12(frame, width, height, out),
		PixelFormat::Nv12 if frame.width as usize == width && frame.height as usize == height => {
			out.clear();
			out.extend_from_slice(&frame.data);
		}
		PixelFormat::Nv12 => scale_nv12(frame, width, height, out),
	}
	Ok(())
}

pub fn nv12_to_i420(data: &[u8], width: usize, height: usize) -> Vec<u8> {
	let planes = split_nv12(data, width, height);
	let quarter = width * height / 4;
	let mut out = Vec::with_capacity(width * height * 3 / 2);
	out.extend_from_slice(planes.luma);
	out.resize(width * height + quarter * 2, 0);
	let (u_plane, v_plane) = out[width * height..].split_at_mut(quarter);
	for ((pair, u), v) in planes.chroma.as_chunks::<2>().0.iter().zip(u_plane).zip(v_plane) {
		*u = pair[0];
		*v = pair[1];
	}
	out
}

pub fn i420_to_nv12(
	luma: &[u8],
	u_plane: &[u8],
	v_plane: &[u8],
	strides: (usize, usize, usize),
	width: usize,
	height: usize,
) -> Vec<u8> {
	let mut out = Vec::with_capacity(width * height * 3 / 2);
	for row in 0..height {
		out.extend_from_slice(&luma[row * strides.0..][..width]);
	}
	for row in 0..height / 2 {
		let u = &u_plane[row * strides.1..][..width / 2];
		let v = &v_plane[row * strides.2..][..width / 2];
		for (u, v) in u.iter().zip(v) {
			out.push(*u);
			out.push(*v);
		}
	}
	out
}

pub fn fit(source_width: u32, source_height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
	let scale = (f64::from(max_width) / f64::from(source_width))
		.min(f64::from(max_height) / f64::from(source_height))
		.min(1.0);
	let even = |value: u32| (((f64::from(value) * scale).round() as u32).max(2)) & !1;
	(even(source_width), even(source_height))
}
