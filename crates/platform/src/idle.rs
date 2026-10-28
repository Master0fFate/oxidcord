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

#[cfg(target_os = "macos")]
pub fn seconds_since_input() -> Option<u64> {
	#[link(name = "CoreGraphics", kind = "framework")]
	unsafe extern "C" {
		fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
	}

	const COMBINED_SESSION_STATE: i32 = 0;
	const ANY_INPUT_EVENT_TYPE: u32 = u32::MAX;
	let seconds =
		unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT_TYPE) };
	(seconds.is_finite() && seconds >= 0.0).then_some(seconds as u64)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn seconds_since_input() -> Option<u64> {
	None
}
