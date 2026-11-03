use crate::Result;

const ALERT_RGBA: [u8; 4] = [0xd2, 0x2d, 0x39, 0xff];
#[cfg(windows)]
const LABEL_RGBA: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
#[cfg(windows)]
const OVERLAY_SIZE: u32 = 32;
#[cfg(windows)]
const GLYPH_WIDTH: u32 = 3;
#[cfg(windows)]
const GLYPH_HEIGHT: u32 = 5;
#[cfg(windows)]
const PLUS: usize = 10;
#[cfg(windows)]
const GLYPHS: [[u8; GLYPH_HEIGHT as usize]; 11] = [
	[0b111, 0b101, 0b101, 0b101, 0b111],
	[0b010, 0b110, 0b010, 0b010, 0b111],
	[0b111, 0b001, 0b111, 0b100, 0b111],
	[0b111, 0b001, 0b111, 0b001, 0b111],
	[0b101, 0b101, 0b111, 0b001, 0b001],
	[0b111, 0b100, 0b111, 0b001, 0b111],
	[0b111, 0b100, 0b111, 0b101, 0b111],
	[0b111, 0b001, 0b001, 0b001, 0b001],
	[0b111, 0b101, 0b111, 0b101, 0b111],
	[0b111, 0b101, 0b111, 0b001, 0b111],
	[0b000, 0b010, 0b111, 0b010, 0b000],
];

pub(crate) fn paint_dot(pixels: &mut [u8], (width, height): (u32, u32), center: (f32, f32), radius: f32) {
	for y in 0..height {
		for x in 0..width {
			let (dx, dy) = (x as f32 + 0.5 - center.0, y as f32 + 0.5 - center.1);
			if dx * dx + dy * dy <= radius * radius {
				let at = ((y * width + x) * 4) as usize;
				pixels[at..at + 4].copy_from_slice(&ALERT_RGBA);
			}
		}
	}
}

#[cfg(windows)]
fn paint_label(pixels: &mut [u8], size: u32, mentions: u32) {
	let (glyphs, count) = match mentions {
		0 => return,
		1..=9 => ([mentions as usize, 0], 1),
		_ => ([9, PLUS], 2),
	};
	let scale = size / if count == 1 { 8 } else { 10 };
	let width = (count as u32 * (GLYPH_WIDTH + 1) - 1) * scale;
	let left = (size - width) / 2;
	let top = (size - GLYPH_HEIGHT * scale) / 2;
	for (index, glyph) in glyphs.into_iter().take(count).enumerate() {
		let glyph_left = left + index as u32 * (GLYPH_WIDTH + 1) * scale;
		for (row, bits) in GLYPHS[glyph].iter().enumerate() {
			for column in 0..GLYPH_WIDTH {
				if bits >> (GLYPH_WIDTH - 1 - column) & 1 == 0 {
					continue;
				}
				for y in 0..scale {
					for x in 0..scale {
						let at = (((top + row as u32 * scale + y) * size + glyph_left + column * scale + x)
							* 4) as usize;
						pixels[at..at + 4].copy_from_slice(&LABEL_RGBA);
					}
				}
			}
		}
	}
}

#[cfg(windows)]
pub fn set_taskbar_mentions(hwnd: isize, mentions: u32, description: &str) -> Result<()> {
	use windows::Win32::Foundation::HWND;
	use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
	use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
	use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
	use windows::core::{HSTRING, PCWSTR};

	let os_error = |error: windows::core::Error| crate::PlatformError::Os(error.to_string());
	let list: ITaskbarList3 =
		unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }.map_err(os_error)?;
	unsafe { list.HrInit() }.map_err(os_error)?;
	let hwnd = HWND(hwnd as *mut _);
	if mentions == 0 {
		return unsafe { list.SetOverlayIcon(hwnd, HICON::default(), PCWSTR::null()) }.map_err(os_error);
	}
	let icon = overlay_icon(mentions).map_err(os_error)?;
	let applied = unsafe { list.SetOverlayIcon(hwnd, icon, &HSTRING::from(description)) };
	let _ = unsafe { DestroyIcon(icon) };
	applied.map_err(os_error)
}

#[cfg(windows)]
fn overlay_icon(mentions: u32) -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HICON> {
	use windows::Win32::UI::WindowsAndMessaging::CreateIcon;

	let size = OVERLAY_SIZE;
	let mut rgba = vec![0u8; (size * size * 4) as usize];
	let middle = size as f32 / 2.;
	paint_dot(&mut rgba, (size, size), (middle, middle), middle);
	paint_label(&mut rgba, size, mentions);
	let bgra: Vec<u8> = rgba
		.as_chunks::<4>()
		.0
		.iter()
		.flat_map(|[red, green, blue, alpha]| [*blue, *green, *red, *alpha])
		.collect();
	let mask = vec![0u8; (size / 8 * size) as usize];
	unsafe {
		CreateIcon(
			None,
			size as i32,
			size as i32,
			1,
			32,
			mask.as_ptr(),
			bgra.as_ptr(),
		)
	}
}

#[cfg(not(windows))]
pub fn set_taskbar_mentions(_: isize, _: u32, _: &str) -> Result<()> {
	Ok(())
}
