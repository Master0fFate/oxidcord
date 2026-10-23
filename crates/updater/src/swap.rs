use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Result, UpdaterError};

pub(crate) struct Replacement {
	pub source: PathBuf,
	pub target: PathBuf,
}

pub(crate) fn sibling(path: &Path, extension: &str) -> PathBuf {
	let mut name = path.as_os_str().to_owned();
	name.push(".");
	name.push(extension);
	PathBuf::from(name)
}

pub(crate) fn backup_path(target: &Path) -> PathBuf {
	sibling(target, "old")
}

pub(crate) fn rename(from: &Path, to: &Path) -> Result<()> {
	fs::rename(from, to).map_err(|error| UpdaterError::io(to, error))
}

pub(crate) fn remove_path(path: &Path) -> Result<()> {
	match fs::symlink_metadata(path) {
		Ok(metadata) if metadata.is_dir() => {
			fs::remove_dir_all(path).map_err(|error| UpdaterError::io(path, error))
		}
		Ok(_) => fs::remove_file(path).map_err(|error| UpdaterError::io(path, error)),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
		Err(error) => Err(UpdaterError::io(path, error)),
	}
}

fn exists(path: &Path) -> bool {
	fs::symlink_metadata(path).is_ok()
}

pub(crate) fn swap_in(replacements: &[Replacement]) -> Result<()> {
	for (index, replacement) in replacements.iter().enumerate() {
		if let Err(error) = swap_one(replacement) {
			for done in replacements[..index].iter().rev() {
				let _ = restore(&done.target);
			}
			return Err(error);
		}
	}
	Ok(())
}

fn swap_one(replacement: &Replacement) -> Result<()> {
	let backup = backup_path(&replacement.target);
	remove_path(&backup)?;
	let had_target = exists(&replacement.target);
	if had_target {
		rename(&replacement.target, &backup)?;
	}
	if let Err(error) = rename(&replacement.source, &replacement.target) {
		if had_target {
			let _ = rename(&backup, &replacement.target);
		}
		return Err(error);
	}
	Ok(())
}

pub(crate) fn restore(target: &Path) -> Result<()> {
	let backup = backup_path(target);
	if !exists(&backup) {
		return remove_path(target);
	}
	let discarded = sibling(target, "bad");
	remove_path(&discarded)?;
	if exists(target) {
		rename(target, &discarded)?;
	}
	rename(&backup, target)?;
	let _ = remove_path(&discarded);
	Ok(())
}

pub(crate) fn discard_backup(target: &Path) {
	let _ = remove_path(&backup_path(target));
}
