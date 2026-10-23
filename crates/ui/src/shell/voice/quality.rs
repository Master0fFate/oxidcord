use session::VideoQuality;

const SOURCE_MAX_WIDTH: u32 = 2560;
const SOURCE_MAX_HEIGHT: u32 = 1440;
const DEFAULT_SOURCE: (u32, u32) = (1920, 1080);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
	Hd720Fps30,
	Hd1080Fps30,
	Hd1080Fps60,
	SourceFps60,
}

impl Preset {
	pub const ALL: [Preset; 4] = [
		Preset::Hd720Fps30,
		Preset::Hd1080Fps30,
		Preset::Hd1080Fps60,
		Preset::SourceFps60,
	];

	pub fn key(self) -> &'static str {
		match self {
			Self::Hd720Fps30 => "voice-quality-720-30",
			Self::Hd1080Fps30 => "voice-quality-1080-30",
			Self::Hd1080Fps60 => "voice-quality-1080-60",
			Self::SourceFps60 => "voice-quality-source-60",
		}
	}

	pub fn index(self) -> u8 {
		Self::ALL.iter().position(|preset| *preset == self).unwrap_or(0) as u8
	}

	pub fn from_index(index: u8) -> Self {
		Self::ALL
			.get(usize::from(index))
			.copied()
			.unwrap_or(Self::Hd720Fps30)
	}

	pub fn quality(self, source: Option<(u32, u32)>) -> VideoQuality {
		match self {
			Self::Hd720Fps30 => VideoQuality {
				width: 1280,
				height: 720,
				fps: 30,
				bitrate: 2_500_000,
			},
			Self::Hd1080Fps30 => VideoQuality {
				width: 1920,
				height: 1080,
				fps: 30,
				bitrate: 4_000_000,
			},
			Self::Hd1080Fps60 => VideoQuality {
				width: 1920,
				height: 1080,
				fps: 60,
				bitrate: 6_000_000,
			},
			Self::SourceFps60 => {
				let (width, height) = source.unwrap_or(DEFAULT_SOURCE);
				VideoQuality {
					width: width.min(SOURCE_MAX_WIDTH) & !1,
					height: height.min(SOURCE_MAX_HEIGHT) & !1,
					fps: 60,
					bitrate: 8_000_000,
				}
			}
		}
	}
}
