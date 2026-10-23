use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::download::{UPDATES_DIR, verify_file};
use crate::error::{Result, UpdaterError};
use crate::swap::{Replacement, discard_backup, remove_path, rename, restore, sibling, swap_in};
use crate::target::{InstallEnv, InstallKind};

pub(crate) const PENDING_DIR: &str = ".update";
pub(crate) const PENDING_MARKER: &str = "pending-update";
const TRIAL_FILE: &str = "trial.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupOutcome {
	Continue,
	Restarted,
	RolledBack,
}

impl StartupOutcome {
	pub fn should_exit(self) -> bool {
		self != Self::Continue
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
	Swapped,
	Started,
}

#[derive(Debug, Serialize, Deserialize)]
struct Trial {
	version: String,
	phase: Phase,
	replaced: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PendingFile {
	pub source: PathBuf,
	pub target: PathBuf,
	pub sha256: String,
	pub length: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Pending {
	pub version: String,
	pub files: Vec<PendingFile>,
}

pub(crate) fn is_safe_relative(path: &Path) -> bool {
	path.components().next().is_some()
		&& path.components().all(|component| match component {
			Component::Normal(part) => !part.to_string_lossy().contains(':'),
			_ => false,
		})
}

fn trial_path(data_dir: &Path) -> PathBuf {
	data_dir.join(UPDATES_DIR).join(TRIAL_FILE)
}

fn read_trial(data_dir: &Path) -> Result<Option<Trial>> {
	let path = trial_path(data_dir);
	match fs::read(&path) {
		Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(UpdaterError::io(path, error)),
	}
}

fn write_trial(data_dir: &Path, trial: &Trial) -> Result<()> {
	let path = trial_path(data_dir);
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).map_err(|error| UpdaterError::io(parent, error))?;
	}
	let temporary = sibling(&path, "tmp");
	fs::write(&temporary, serde_json::to_vec(trial)?).map_err(|error| UpdaterError::io(&temporary, error))?;
	rename(&temporary, &path)
}

pub(crate) fn begin_trial(data_dir: &Path, version: &str, replaced: Vec<PathBuf>) -> Result<()> {
	let trial = Trial {
		version: version.to_owned(),
		phase: Phase::Swapped,
		replaced,
	};
	if let Err(error) = write_trial(data_dir, &trial) {
		for target in &trial.replaced {
			let _ = restore(target);
		}
		return Err(error);
	}
	Ok(())
}

fn finish_trial(data_dir: &Path) -> Result<()> {
	remove_path(&trial_path(data_dir))
}

fn restore_trial(data_dir: &Path, trial: &Trial) -> Result<()> {
	let mut first_error = None;
	for target in &trial.replaced {
		if let Err(error) = restore(target) {
			first_error.get_or_insert(error);
		}
	}
	match first_error {
		Some(error) => Err(error),
		None => finish_trial(data_dir),
	}
}

pub fn apply_pending_in(env: &InstallEnv, data_dir: &Path) -> Result<StartupOutcome> {
	let _ = remove_path(&sibling(&env.app_path, "bad"));
	let pending_dir = env.exe.parent().map(|dir| dir.join(PENDING_DIR));
	if let Some(trial) = read_trial(data_dir)? {
		return match trial.phase {
			Phase::Swapped => {
				write_trial(
					data_dir,
					&Trial {
						phase: Phase::Started,
						..trial
					},
				)?;
				Ok(StartupOutcome::Continue)
			}
			Phase::Started => {
				restore_trial(data_dir, &trial)?;
				if let Some(dir) = &pending_dir {
					let _ = remove_path(dir);
				}
				tracing::warn!(version = %trial.version, "previous update did not start cleanly, rolled back");
				Ok(StartupOutcome::RolledBack)
			}
		};
	}
	let Some(dir) = pending_dir.filter(|dir| env.kind == Some(InstallKind::Portable) && dir.is_dir()) else {
		return Ok(StartupOutcome::Continue);
	};
	let outcome = apply_pending(env, data_dir, &dir);
	let _ = remove_path(&dir);
	outcome
}

fn apply_pending(env: &InstallEnv, data_dir: &Path, dir: &Path) -> Result<StartupOutcome> {
	let marker = dir.join(PENDING_MARKER);
	let bytes = match fs::read(&marker) {
		Ok(bytes) => bytes,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(StartupOutcome::Continue),
		Err(error) => return Err(UpdaterError::io(marker, error)),
	};
	let pending: Pending = serde_json::from_slice(&bytes)?;
	let exe_dir = env
		.exe
		.parent()
		.ok_or_else(|| UpdaterError::Install("executable has no directory".to_owned()))?;
	let mut replacements = Vec::with_capacity(pending.files.len());
	for file in &pending.files {
		if !is_safe_relative(&file.source) || !is_safe_relative(&file.target) {
			return Err(UpdaterError::UnsafeArchive(file.target.display().to_string()));
		}
		let source = dir.join(&file.source);
		verify_file(&source, &file.sha256, file.length)?;
		let target = exe_dir.join(&file.target);
		if let Some(parent) = target.parent() {
			fs::create_dir_all(parent).map_err(|error| UpdaterError::io(parent, error))?;
		}
		replacements.push(Replacement { source, target });
	}
	swap_in(&replacements)?;
	let replaced = replacements
		.into_iter()
		.map(|replacement| replacement.target)
		.collect();
	begin_trial(data_dir, &pending.version, replaced)?;
	Ok(StartupOutcome::Restarted)
}

pub fn mark_start_ok_in(data_dir: &Path) -> Result<()> {
	let Some(trial) = read_trial(data_dir)? else {
		return Ok(());
	};
	for target in &trial.replaced {
		discard_backup(target);
	}
	finish_trial(data_dir)?;
	let _ = remove_path(&data_dir.join(UPDATES_DIR).join(&trial.version));
	Ok(())
}

pub fn rollback_in(data_dir: &Path) -> Result<bool> {
	let Some(trial) = read_trial(data_dir)? else {
		return Ok(false);
	};
	restore_trial(data_dir, &trial)?;
	Ok(true)
}

pub fn apply_pending_on_start(data_dir: &Path) -> Result<StartupOutcome> {
	let env = InstallEnv::detect()?;
	let outcome = apply_pending_in(&env, data_dir)?;
	if outcome.should_exit() {
		relaunch(&env)?;
	}
	Ok(outcome)
}

pub fn mark_start_ok(data_dir: &Path) -> Result<()> {
	mark_start_ok_in(data_dir)
}

pub fn rollback(data_dir: &Path) -> Result<bool> {
	rollback_in(data_dir)
}

fn relaunch(env: &InstallEnv) -> Result<()> {
	let mut command = if env.kind == Some(InstallKind::Dmg) {
		let mut command = Command::new("open");
		command.arg("-n").arg(&env.app_path);
		command
	} else {
		let mut command = Command::new(&env.app_path);
		command.args(std::env::args_os().skip(1));
		command
	};
	command
		.spawn()
		.map(drop)
		.map_err(|error| UpdaterError::io(&env.app_path, error))
}
