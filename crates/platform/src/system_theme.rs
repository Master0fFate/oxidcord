#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
	Dark,
	Light,
}

#[cfg(windows)]
pub fn current() -> SystemTheme {
	use std::ffi::c_void;

	use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
	use windows::core::w;

	let mut uses_light = 0u32;
	let mut size = size_of::<u32>() as u32;
	let status = unsafe {
		RegGetValueW(
			HKEY_CURRENT_USER,
			w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
			w!("AppsUseLightTheme"),
			RRF_RT_REG_DWORD,
			None,
			Some(&mut uses_light as *mut u32 as *mut c_void),
			Some(&mut size),
		)
	};
	if status.is_ok() && uses_light != 0 {
		SystemTheme::Light
	} else {
		SystemTheme::Dark
	}
}

#[cfg(target_os = "macos")]
pub fn current() -> SystemTheme {
	match std::process::Command::new("defaults")
		.args(["read", "-g", "AppleInterfaceStyle"])
		.output()
	{
		Ok(output) if output.status.success() => SystemTheme::Dark,
		Ok(_) => SystemTheme::Light,
		Err(_) => SystemTheme::Dark,
	}
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn current() -> SystemTheme {
	let output = std::process::Command::new("gsettings")
		.args(["get", "org.gnome.desktop.interface", "color-scheme"])
		.output();
	match output {
		Ok(output) if output.status.success() => {
			if String::from_utf8_lossy(&output.stdout).contains("prefer-dark") {
				SystemTheme::Dark
			} else {
				SystemTheme::Light
			}
		}
		_ => SystemTheme::Dark,
	}
}
