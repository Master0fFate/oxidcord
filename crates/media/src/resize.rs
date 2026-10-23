use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::types::{Animation, Decoded, DecodedImage, Frame};

pub fn resize_to_fit(image: &DecodedImage, max_w: u32, max_h: u32) -> DecodedImage {
	match fit_dimensions(image.width, image.height, max_w, max_h) {
		Some((width, height)) => resize_exact(&mut Resizer::new(), image, width, height),
		None => image.clone(),
	}
}

pub fn resize_decoded(decoded: Decoded, max_w: u32, max_h: u32) -> Decoded {
	let (width, height) = decoded.dimensions();
	let Some((target_w, target_h)) = fit_dimensions(width, height, max_w, max_h) else {
		return decoded;
	};
	let mut resizer = Resizer::new();
	match decoded {
		Decoded::Still(image) => Decoded::Still(resize_exact(&mut resizer, &image, target_w, target_h)),
		Decoded::Animated(animation) => Decoded::Animated(Animation {
			loops: animation.loops,
			frames: animation
				.frames
				.into_iter()
				.map(|frame| Frame {
					image: resize_exact(&mut resizer, &frame.image, target_w, target_h),
					delay_ms: frame.delay_ms,
				})
				.collect(),
		}),
	}
}

fn fit_dimensions(width: u32, height: u32, max_w: u32, max_h: u32) -> Option<(u32, u32)> {
	if width == 0 || height == 0 || max_w == 0 || max_h == 0 || (width <= max_w && height <= max_h) {
		return None;
	}
	let scale = (f64::from(max_w) / f64::from(width)).min(f64::from(max_h) / f64::from(height));
	let fitted_w = ((f64::from(width) * scale).round() as u32).clamp(1, max_w);
	let fitted_h = ((f64::from(height) * scale).round() as u32).clamp(1, max_h);
	Some((fitted_w, fitted_h))
}

fn resize_exact(resizer: &mut Resizer, image: &DecodedImage, width: u32, height: u32) -> DecodedImage {
	let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
	let resized = ImageRef::new(image.width, image.height, &image.rgba, PixelType::U8x4)
		.ok()
		.and_then(|source| {
			let mut target = Image::new(width, height, PixelType::U8x4);
			resizer.resize(&source, &mut target, &options).ok()?;
			Some(target)
		});
	match resized {
		Some(target) => DecodedImage {
			width,
			height,
			rgba: target.into_vec(),
		},
		None => image.clone(),
	}
}
