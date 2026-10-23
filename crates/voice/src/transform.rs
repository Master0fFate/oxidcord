use crate::dave::DaveTransform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
	Opus,
	H264,
}

pub trait FrameTransform: Send {
	fn encrypt(&mut self, codec: Codec, frame: &[u8]) -> Option<Vec<u8>>;
	fn decrypt(&mut self, codec: Codec, ssrc: u32, payload: &[u8]) -> Option<Vec<u8>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Passthrough;

impl FrameTransform for Passthrough {
	fn encrypt(&mut self, _codec: Codec, frame: &[u8]) -> Option<Vec<u8>> {
		Some(frame.to_vec())
	}

	fn decrypt(&mut self, _codec: Codec, _ssrc: u32, payload: &[u8]) -> Option<Vec<u8>> {
		Some(payload.to_vec())
	}
}

pub(crate) struct Layered {
	dave: DaveTransform,
	inner: Box<dyn FrameTransform>,
}

impl Layered {
	pub fn new(dave: DaveTransform, inner: Box<dyn FrameTransform>) -> Self {
		Self { dave, inner }
	}
}

impl FrameTransform for Layered {
	fn encrypt(&mut self, codec: Codec, frame: &[u8]) -> Option<Vec<u8>> {
		let inner = self.inner.encrypt(codec, frame)?;
		self.dave.encrypt(codec, &inner)
	}

	fn decrypt(&mut self, codec: Codec, ssrc: u32, payload: &[u8]) -> Option<Vec<u8>> {
		let opened = self.dave.decrypt(codec, ssrc, payload)?;
		self.inner.decrypt(codec, ssrc, &opened)
	}
}
