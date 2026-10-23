use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, UpdaterError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
	#[serde(rename = "windows-x86_64")]
	WindowsX86_64,
	#[serde(rename = "macos-aarch64")]
	MacosAarch64,
	#[serde(rename = "macos-x86_64")]
	MacosX86_64,
	#[serde(rename = "linux-x86_64")]
	LinuxX86_64,
	#[serde(rename = "linux-aarch64")]
	LinuxAarch64,
	#[serde(other)]
	Unknown,
}

impl Target {
	pub const ALL: [Self; 5] = [
		Self::WindowsX86_64,
		Self::MacosAarch64,
		Self::MacosX86_64,
		Self::LinuxX86_64,
		Self::LinuxAarch64,
	];

	pub fn current() -> Option<Self> {
		match (std::env::consts::OS, std::env::consts::ARCH) {
			("windows", "x86_64") => Some(Self::WindowsX86_64),
			("macos", "aarch64") => Some(Self::MacosAarch64),
			("macos", "x86_64") => Some(Self::MacosX86_64),
			("linux", "x86_64") => Some(Self::LinuxX86_64),
			("linux", "aarch64") => Some(Self::LinuxAarch64),
			_ => None,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::WindowsX86_64 => "windows-x86_64",
			Self::MacosAarch64 => "macos-aarch64",
			Self::MacosX86_64 => "macos-x86_64",
			Self::LinuxX86_64 => "linux-x86_64",
			Self::LinuxAarch64 => "linux-aarch64",
			Self::Unknown => "unknown",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
	Installer,
	Portable,
	Appimage,
	Dmg,
	Deb,
	Rpm,
	#[serde(other)]
	Unknown,
}

impl ArtifactKind {
	pub const ALL: [Self; 6] = [
		Self::Installer,
		Self::Portable,
		Self::Appimage,
		Self::Dmg,
		Self::Deb,
		Self::Rpm,
	];

	pub fn as_str(self) -> &'static str {
		match self {
			Self::Installer => "installer",
			Self::Portable => "portable",
			Self::Appimage => "appimage",
			Self::Dmg => "dmg",
			Self::Deb => "deb",
			Self::Rpm => "rpm",
			Self::Unknown => "unknown",
		}
	}

	pub fn extension(self) -> &'static str {
		match self {
			Self::Installer => "exe",
			Self::Portable => "zip",
			Self::Appimage => "AppImage",
			Self::Dmg => "dmg",
			Self::Deb => "deb",
			Self::Rpm => "rpm",
			Self::Unknown => "bin",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
	#[default]
	Stable,
	Beta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
	Installer,
	Portable,
	AppImage,
	Dmg,
}

impl InstallKind {
	pub fn artifact_kind(self) -> ArtifactKind {
		match self {
			Self::Installer => ArtifactKind::Installer,
			Self::Portable => ArtifactKind::Portable,
			Self::AppImage => ArtifactKind::Appimage,
			Self::Dmg => ArtifactKind::Dmg,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallEnv {
	pub kind: Option<InstallKind>,
	pub exe: PathBuf,
	pub app_path: PathBuf,
}

impl InstallEnv {
	pub fn detect() -> Result<Self> {
		let exe = std::env::current_exe().map_err(|error| UpdaterError::io("current exe", error))?;
		Ok(if cfg!(windows) {
			Self::windows(exe, std::env::var_os("LOCALAPPDATA"))
		} else if cfg!(target_os = "macos") {
			Self::macos(exe)
		} else {
			Self::linux(exe, std::env::var_os("APPIMAGE"))
		})
	}

	pub fn windows(exe: PathBuf, local_app_data: Option<OsString>) -> Self {
		let installed = local_app_data.is_some_and(|base| {
			let base = Path::new(&base).join("Programs").join("Oxidcord");
			Path::new(&lowercase(&exe)).starts_with(lowercase(&base))
		});
		let kind = if installed {
			InstallKind::Installer
		} else {
			InstallKind::Portable
		};
		Self {
			kind: Some(kind),
			app_path: exe.clone(),
			exe,
		}
	}

	pub fn macos(exe: PathBuf) -> Self {
		let bundle = exe
			.ancestors()
			.find(|path| path.extension().is_some_and(|extension| extension == "app"))
			.map(Path::to_path_buf);
		Self {
			kind: bundle.as_ref().map(|_| InstallKind::Dmg),
			app_path: bundle.unwrap_or_else(|| exe.clone()),
			exe,
		}
	}

	pub fn linux(exe: PathBuf, appimage: Option<OsString>) -> Self {
		match appimage.filter(|path| !path.is_empty()) {
			Some(path) => Self {
				kind: Some(InstallKind::AppImage),
				app_path: PathBuf::from(path),
				exe,
			},
			None => Self {
				kind: None,
				app_path: exe.clone(),
				exe,
			},
		}
	}
}

fn lowercase(path: &Path) -> String {
	path.to_string_lossy().to_lowercase()
}
