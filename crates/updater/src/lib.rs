mod download;
mod error;
mod install;
pub mod keys;
pub mod manifest;
mod net;
mod pending;
mod swap;
mod target;
mod updater;
mod verify;
mod version;

pub use download::Staged;
pub use error::{Result, UpdaterError};
pub use install::InstallOutcome;
pub use manifest::{Artifact, ReleaseManifest};
pub use pending::{
	StartupOutcome, apply_pending_in, apply_pending_on_start, mark_start_ok, mark_start_ok_in, rollback,
	rollback_in,
};
pub use target::{ArtifactKind, Channel, InstallEnv, InstallKind, Target};
pub use updater::{AvailableUpdate, Updater, UpdaterConfig};
pub use version::{VersionInfo, version_info};
