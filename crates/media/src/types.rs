#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
	pub image: DecodedImage,
	pub delay_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Animation {
	pub frames: Vec<Frame>,
	pub loops: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
	Still(DecodedImage),
	Animated(Animation),
}

impl Decoded {
	pub fn dimensions(&self) -> (u32, u32) {
		match self {
			Self::Still(image) => (image.width, image.height),
			Self::Animated(animation) => {
				let first = &animation.frames[0].image;
				(first.width, first.height)
			}
		}
	}

	pub fn byte_len(&self) -> usize {
		match self {
			Self::Still(image) => image.rgba.len(),
			Self::Animated(animation) => animation.frames.iter().map(|frame| frame.image.rgba.len()).sum(),
		}
	}
}
