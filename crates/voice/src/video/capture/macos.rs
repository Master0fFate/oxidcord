use super::{CameraInfo, ScreenTarget, VideoSource};
use crate::video::{VideoError, VideoFrame};

pub struct CameraSource;

impl CameraSource {
	pub fn list() -> Result<Vec<CameraInfo>, VideoError> {
		Err(VideoError::Unsupported)
	}

	pub fn open(_id: &str, _width: u32, _height: u32, _fps: u32) -> Result<Self, VideoError> {
		Err(VideoError::Unsupported)
	}
}

impl VideoSource for CameraSource {
	fn next_frame(&mut self) -> Result<VideoFrame, VideoError> {
		Err(VideoError::Unsupported)
	}

	fn size(&self) -> (u32, u32) {
		(0, 0)
	}
}

pub struct ScreenSource;

impl ScreenSource {
	pub fn list() -> Result<Vec<ScreenTarget>, VideoError> {
		Err(VideoError::Unsupported)
	}

	pub fn open(_target: &ScreenTarget, _fps: u32) -> Result<Self, VideoError> {
		Err(VideoError::Unsupported)
	}
}

impl VideoSource for ScreenSource {
	fn next_frame(&mut self) -> Result<VideoFrame, VideoError> {
		Err(VideoError::Unsupported)
	}

	fn size(&self) -> (u32, u32) {
		(0, 0)
	}
}
