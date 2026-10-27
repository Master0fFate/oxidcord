use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::error::Result;

pub fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<()> {
	let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
	temp_name.push(".tmp");
	let temp_path = path.with_file_name(temp_name);
	let mut file = create(&temp_path, private)?;
	file.write_all(bytes)?;
	file.sync_all()?;
	drop(file);
	fs::rename(&temp_path, path).inspect_err(|_| {
		let _ = fs::remove_file(&temp_path);
	})?;
	Ok(())
}

#[cfg(unix)]
fn create(path: &Path, private: bool) -> std::io::Result<File> {
	use std::os::unix::fs::OpenOptionsExt;

	let mode = if private { 0o600 } else { 0o644 };
	OpenOptions::new()
		.write(true)
		.create(true)
		.truncate(true)
		.mode(mode)
		.open(path)
}

#[cfg(windows)]
fn create(path: &Path, private: bool) -> std::io::Result<File> {
	use std::os::windows::fs::OpenOptionsExt;

	use windows::Win32::Foundation::GENERIC_WRITE;
	use windows::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};

	let mut options = OpenOptions::new();
	options.write(true).create(true).truncate(true);
	if !private {
		return options.open(path);
	}
	let file = options
		.access_mode(GENERIC_WRITE.0 | WRITE_DAC.0 | READ_CONTROL.0)
		.open(path)?;
	crate::acl::restrict_to_current_user(&file)?;
	Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn create(path: &Path, _private: bool) -> std::io::Result<File> {
	OpenOptions::new()
		.write(true)
		.create(true)
		.truncate(true)
		.open(path)
}
