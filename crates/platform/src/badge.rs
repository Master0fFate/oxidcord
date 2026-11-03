use crate::Result;

const ALERT_RGBA: [u8; 4] = [0xd2, 0x2d, 0x39, 0xff];
#[cfg(windows)]
const OVERLAY_SIZE: u32 = 16;

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
	let icon = overlay_icon().map_err(os_error)?;
	let applied = unsafe { list.SetOverlayIcon(hwnd, icon, &HSTRING::from(description)) };
	let _ = unsafe { DestroyIcon(icon) };
	applied.map_err(os_error)
}

#[cfg(windows)]
fn overlay_icon() -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HICON> {
	use windows::Win32::UI::WindowsAndMessaging::CreateIcon;

	let size = OVERLAY_SIZE;
	let mut rgba = vec![0u8; (size * size * 4) as usize];
	let middle = size as f32 / 2.;
	paint_dot(&mut rgba, (size, size), (middle, middle), middle);
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
