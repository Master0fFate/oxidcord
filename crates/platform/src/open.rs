use std::path::Path;
use std::process::Command;

use url::Url;

use crate::{PlatformError, Result};

const ALLOWED_SCHEMES: [&str; 2] = ["http", "https"];

pub fn open_url(raw: &str) -> Result<()> {
	let url = Url::parse(raw)?;
	if !ALLOWED_SCHEMES.contains(&url.scheme()) {
		return Err(PlatformError::UnsupportedScheme(url.scheme().to_owned()));
	}
	launch(url.as_str())
}

#[cfg(windows)]
fn launch(target: &str) -> Result<()> {
	use windows::Win32::UI::Shell::ShellExecuteW;
	use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
	use windows::core::{HSTRING, w};

	let result = unsafe {
		ShellExecuteW(
			None,
			w!("open"),
			&HSTRING::from(target),
			None,
			None,
			SW_SHOWNORMAL,
		)
	};
	if result.0 as isize > 32 {
		Ok(())
	} else {
		Err(PlatformError::Os(format!(
			"ShellExecuteW failed with code {}",
			result.0 as isize
		)))
	}
}

#[cfg(target_os = "macos")]
fn launch(target: &str) -> Result<()> {
	run("open", &[target])
}

#[cfg(not(any(windows, target_os = "macos")))]
fn launch(target: &str) -> Result<()> {
	run("xdg-open", &[target])
}

#[cfg(windows)]
pub fn reveal_path(path: &Path) -> Result<()> {
	use std::os::windows::process::CommandExt;

	Command::new("explorer")
		.raw_arg(format!("/select,\"{}\"", path.display()))
		.spawn()?;
	Ok(())
}

#[cfg(target_os = "macos")]
pub fn reveal_path(path: &Path) -> Result<()> {
	run("open", &["-R".as_ref(), path.as_os_str()])
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn reveal_path(path: &Path) -> Result<()> {
	run("xdg-open", &[path.parent().unwrap_or(path).as_os_str()])
}

#[cfg(not(windows))]
fn run<S: AsRef<std::ffi::OsStr>>(program: &str, args: &[S]) -> Result<()> {
	let status = Command::new(program).args(args).status()?;
	if status.success() {
		Ok(())
	} else {
		Err(PlatformError::Os(format!("{program} exited with {status}")))
	}
}
