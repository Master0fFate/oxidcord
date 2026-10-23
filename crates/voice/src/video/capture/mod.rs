#[cfg(not(any(windows, target_os = "macos")))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win32;

#[cfg(not(any(windows, target_os = "macos")))]
pub use linux::{CameraSource, ScreenSource};
#[cfg(target_os = "macos")]
pub use macos::{CameraSource, ScreenSource};
#[cfg(windows)]
pub use win32::{CameraSource, ScreenSource};

use super::{VideoError, VideoFrame};

pub trait VideoSource: Send {
	fn next_frame(&mut self) -> Result<VideoFrame, VideoError>;

	fn size(&self) -> (u32, u32);

	fn set_fps(&mut self, _fps: u32) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraInfo {
	pub id: String,
	pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenTargetKind {
	Monitor,
	Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenTarget {
	pub kind: ScreenTargetKind,
	pub id: u64,
	pub name: String,
	pub width: u32,
	pub height: u32,
	pub primary: bool,
}
