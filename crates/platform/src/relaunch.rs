use std::ffi::OsString;
use std::process::Command;

use crate::Result;

pub const FLAG: &str = "--relaunch";

pub fn current_exe() -> Result<()> {
	let exe = std::env::current_exe()?;
	let args: Vec<OsString> = std::env::args_os().skip(1).filter(|arg| arg != FLAG).collect();
	Command::new(exe).args(args).arg(FLAG).spawn()?;
	Ok(())
}
