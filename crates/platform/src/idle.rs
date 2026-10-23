#[cfg(windows)]
pub fn seconds_since_input() -> Option<u64> {
	use windows::Win32::System::SystemInformation::GetTickCount;
	use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

	let mut info = LASTINPUTINFO {
		cbSize: size_of::<LASTINPUTINFO>() as u32,
		dwTime: 0,
	};
	if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
		return None;
	}
	let idle_millis = unsafe { GetTickCount() }.wrapping_sub(info.dwTime);
	Some(u64::from(idle_millis) / 1000)
}

#[cfg(not(windows))]
pub fn seconds_since_input() -> Option<u64> {
	None
}
