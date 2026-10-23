use qrcode::{Color, QrCode};

use crate::error::{LoginError, Result};

const MODULE_PIXELS: usize = 6;
const QUIET_ZONE_MODULES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrBitmap {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

impl QrBitmap {
	pub fn render(payload: &str) -> Result<Self> {
		let code = QrCode::new(payload.as_bytes())
			.map_err(|_| LoginError::Protocol("qr code could not be encoded".to_owned()))?;
		let modules = code.width();
		let colors = code.into_colors();
		let side = (modules + 2 * QUIET_ZONE_MODULES) * MODULE_PIXELS;
		let mut rgba = vec![0xff; side * side * 4];
		for (index, color) in colors.iter().enumerate() {
			if *color == Color::Light {
				continue;
			}
			let left = (index % modules + QUIET_ZONE_MODULES) * MODULE_PIXELS;
			let top = (index / modules + QUIET_ZONE_MODULES) * MODULE_PIXELS;
			for row in top..top + MODULE_PIXELS {
				for column in left..left + MODULE_PIXELS {
					let at = (row * side + column) * 4;
					rgba[at..at + 3].fill(0);
				}
			}
		}
		Ok(Self {
			width: side as u32,
			height: side as u32,
			rgba,
		})
	}
}
