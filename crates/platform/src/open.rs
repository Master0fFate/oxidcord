use std::path::Path;
use std::process::Command;

use url::Url;

use crate::{PlatformError, Result};

const ALLOWED_SCHEMES: [&str; 2] = ["http", "https"];
const APP_SCHEMES: [&str; 5] = ["spotify", "steam", "tidal", "itms-apps", "music"];

pub fn open_url(raw: &str) -> Result<()> {
	open_allowed(raw, &[])
}

pub fn open_plugin_url(raw: &str) -> Result<()> {
	open_allowed(raw, &APP_SCHEMES)
}

fn open_allowed(raw: &str, extra: &[&str]) -> Result<()> {
	let url = Url::parse(raw)?;
	let scheme = url.scheme();
	if !ALLOWED_SCHEMES.contains(&scheme) && !extra.contains(&scheme) {
		return Err(PlatformError::UnsupportedScheme(scheme.to_owned()));
	}
	launch(url.as_str())
}

#[cfg(windows)]
fn launch(target: &str) -> Result<()> {
	let target = target.to_owned();
	std::thread::Builder::new()
		.name("open-url".into())
		.spawn(move || {
			if let Err(error) = shell_execute(&target) {
				tracing::warn!(%error, "could not open the link");
			}
		})?;
	Ok(())
}

#[cfg(windows)]
fn shell_execute(target: &str) -> Result<()> {
	use windows::Win32::System::Com::{
		COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
	};
	use windows::Win32::UI::Shell::ShellExecuteW;
	use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
	use windows::core::{HSTRING, w};

	unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
		.ok()
		.map_err(|error| PlatformError::Os(error.to_string()))?;
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
	unsafe { CoUninitialize() };
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
