use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::download::Staged;
use crate::error::{Result, UpdaterError};
use crate::pending::{PENDING_DIR, PENDING_MARKER, Pending, PendingFile, begin_trial, is_safe_relative};
use crate::swap::{Replacement, remove_path, sibling, swap_in};
use crate::target::{InstallEnv, InstallKind};

const MAX_EXTRACT_BYTES: u64 = 1024 * 1024 * 1024;
const MAIN_EXE: &str = "oxidcord.exe";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
	RestartRequired,
}

pub(crate) fn install(env: &InstallEnv, data_dir: &Path, staged: &Staged) -> Result<InstallOutcome> {
	let kind = env.kind.ok_or(UpdaterError::CheckOnly)?;
	if kind.artifact_kind() != staged.kind {
		return Err(UpdaterError::Install(
			"staged package does not match this install".to_owned(),
		));
	}
	staged.verify()?;
	match kind {
		InstallKind::Installer => run_installer(&staged.path)?,
		InstallKind::Portable => stage_portable(env, staged)?,
		InstallKind::AppImage => replace_appimage(env, data_dir, staged)?,
		InstallKind::Dmg => replace_bundle(env, data_dir, staged)?,
	}
	Ok(InstallOutcome::RestartRequired)
}

#[cfg(windows)]
fn run_installer(path: &Path) -> Result<()> {
	use std::os::windows::process::CommandExt;
	use std::process::Stdio;

	const DETACHED_PROCESS: u32 = 0x0000_0008;
	const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
	const CREATE_NO_WINDOW: u32 = 0x0800_0000;
	let text = path
		.to_str()
		.filter(|text| !text.contains(['"', '%', '&', '^', '|', '<', '>', '!']))
		.ok_or_else(|| UpdaterError::Install("installer path has unsupported characters".to_owned()))?;
	Command::new("cmd")
		.raw_arg(format!(
			"/d /s /c \"ping -n 4 127.0.0.1 >nul & start \"\" /wait \"{text}\" /S /R\""
		))
		.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.spawn()
		.map(drop)
		.map_err(|error| UpdaterError::io(path, error))
}

#[cfg(not(windows))]
fn run_installer(_path: &Path) -> Result<()> {
	Err(UpdaterError::Install(
		"installer updates are only supported on Windows".to_owned(),
	))
}

fn stage_portable(env: &InstallEnv, staged: &Staged) -> Result<()> {
	let exe_dir = env
		.exe
		.parent()
		.ok_or_else(|| UpdaterError::Install("executable has no directory".to_owned()))?;
	let dir = exe_dir.join(PENDING_DIR);
	remove_path(&dir)?;
	fs::create_dir_all(&dir).map_err(|error| UpdaterError::io(&dir, error))?;
	let outcome = extract_portable(env, staged, &dir).and_then(|pending| {
		let marker = dir.join(PENDING_MARKER);
		fs::write(&marker, serde_json::to_vec(&pending)?).map_err(|error| UpdaterError::io(marker, error))
	});
	if outcome.is_err() {
		let _ = remove_path(&dir);
	}
	outcome
}

fn extract_portable(env: &InstallEnv, staged: &Staged, dir: &Path) -> Result<Pending> {
	let exe_name = env
		.exe
		.file_name()
		.map(PathBuf::from)
		.ok_or_else(|| UpdaterError::Install("executable has no file name".to_owned()))?;
	let file = File::open(&staged.path).map_err(|error| UpdaterError::io(&staged.path, error))?;
	let mut archive = ZipArchive::new(BufReader::new(file))?;
	let mut budget = MAX_EXTRACT_BYTES;
	let mut files = Vec::new();
	let mut has_main_exe = false;
	for index in 0..archive.len() {
		let mut entry = archive.by_index(index)?;
		let name = String::from_utf8_lossy(entry.name_raw()).into_owned();
		let relative = entry
			.enclosed_name()
			.filter(|path| is_safe_relative(path) && !is_reserved(path))
			.filter(|_| !entry.is_symlink())
			.ok_or_else(|| UpdaterError::UnsafeArchive(name.clone()))?;
		if entry.is_dir() {
			continue;
		}
		let size = entry.size();
		budget = budget
			.checked_sub(size)
			.ok_or_else(|| UpdaterError::Install("archive is too large".to_owned()))?;
		let destination = dir.join(&relative);
		if let Some(parent) = destination.parent() {
			fs::create_dir_all(parent).map_err(|error| UpdaterError::io(parent, error))?;
		}
		let (length, sha256) = copy_hashed(&mut entry, &destination, size)?;
		let is_main_exe = relative.as_os_str().eq_ignore_ascii_case(MAIN_EXE);
		has_main_exe |= is_main_exe;
		files.push(PendingFile {
			target: if is_main_exe {
				exe_name.clone()
			} else {
				relative.clone()
			},
			source: relative,
			sha256,
			length,
		});
	}
	if !has_main_exe {
		return Err(UpdaterError::Install(format!("archive has no {MAIN_EXE}")));
	}
	Ok(Pending {
		version: staged.version.to_string(),
		files,
	})
}

fn is_reserved(path: &Path) -> bool {
	path.components()
		.next()
		.is_some_and(|first| first.as_os_str().eq_ignore_ascii_case(PENDING_DIR))
}

fn copy_hashed(reader: &mut impl Read, destination: &Path, declared: u64) -> Result<(u64, String)> {
	let mut output = File::create(destination).map_err(|error| UpdaterError::io(destination, error))?;
	let mut limited = reader.by_ref().take(declared + 1);
	let mut hasher = Sha256::new();
	let mut buffer = vec![0u8; 64 * 1024];
	let mut total = 0u64;
	loop {
		let read = limited
			.read(&mut buffer)
			.map_err(|error| UpdaterError::io(destination, error))?;
		if read == 0 {
			break;
		}
		total += read as u64;
		if total > declared {
			return Err(UpdaterError::Install(
				"archive entry is larger than declared".to_owned(),
			));
		}
		hasher.update(&buffer[..read]);
		output
			.write_all(&buffer[..read])
			.map_err(|error| UpdaterError::io(destination, error))?;
	}
	output
		.sync_all()
		.map_err(|error| UpdaterError::io(destination, error))?;
	Ok((total, hex::encode(hasher.finalize())))
}

fn replace_appimage(env: &InstallEnv, data_dir: &Path, staged: &Staged) -> Result<()> {
	stage_and_swap(env, data_dir, staged, |incoming| {
		fs::copy(&staged.path, incoming).map_err(|error| UpdaterError::io(incoming, error))?;
		set_executable(incoming)
	})
}

fn replace_bundle(env: &InstallEnv, data_dir: &Path, staged: &Staged) -> Result<()> {
	let mount_point = staged
		.path
		.parent()
		.map(|dir| dir.join("mount"))
		.ok_or_else(|| UpdaterError::Install("staged package has no directory".to_owned()))?;
	stage_and_swap(env, data_dir, staged, |incoming| {
		copy_bundle_from_dmg(&staged.path, incoming, &mount_point)
	})
}

fn stage_and_swap(
	env: &InstallEnv,
	data_dir: &Path,
	staged: &Staged,
	prepare: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
	let target = env.app_path.clone();
	let incoming = sibling(&target, "new");
	remove_path(&incoming)?;
	let swapped = prepare(&incoming).and_then(|()| {
		swap_in(&[Replacement {
			source: incoming.clone(),
			target: target.clone(),
		}])
	});
	if let Err(error) = swapped {
		let _ = remove_path(&incoming);
		return Err(error);
	}
	begin_trial(data_dir, &staged.version.to_string(), vec![target])
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
	use std::os::unix::fs::PermissionsExt;

	fs::set_permissions(path, fs::Permissions::from_mode(0o755))
		.map_err(|error| UpdaterError::io(path, error))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
	Ok(())
}

fn copy_bundle_from_dmg(dmg: &Path, destination: &Path, mount_point: &Path) -> Result<()> {
	fs::create_dir_all(mount_point).map_err(|error| UpdaterError::io(mount_point, error))?;
	run(
		"hdiutil",
		&[
			OsStr::new("attach"),
			OsStr::new("-nobrowse"),
			OsStr::new("-readonly"),
			OsStr::new("-mountpoint"),
			mount_point.as_os_str(),
			dmg.as_os_str(),
		],
	)?;
	let copied = find_bundle(mount_point)
		.and_then(|bundle| run("ditto", &[bundle.as_os_str(), destination.as_os_str()]));
	if run("hdiutil", &[OsStr::new("detach"), mount_point.as_os_str()]).is_err() {
		let _ = run(
			"hdiutil",
			&[
				OsStr::new("detach"),
				OsStr::new("-force"),
				mount_point.as_os_str(),
			],
		);
	}
	let _ = fs::remove_dir(mount_point);
	copied
}

fn find_bundle(mount_point: &Path) -> Result<PathBuf> {
	let entries = fs::read_dir(mount_point).map_err(|error| UpdaterError::io(mount_point, error))?;
	entries
		.filter_map(std::result::Result::ok)
		.map(|entry| entry.path())
		.find(|path| path.is_dir() && path.extension().is_some_and(|extension| extension == "app"))
		.ok_or_else(|| UpdaterError::Install("disk image contains no app bundle".to_owned()))
}

fn run(program: &str, args: &[&OsStr]) -> Result<()> {
	let status = Command::new(program)
		.args(args)
		.output()
		.map_err(|error| UpdaterError::io(program, error))?
		.status;
	if status.success() {
		Ok(())
	} else {
		Err(UpdaterError::Install(format!("{program} exited with {status}")))
	}
}
