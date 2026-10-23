mod bindings;
mod broker;
pub mod catalog;
pub mod config;
pub mod conformance;
pub mod convert;
mod error;
mod fetch;
mod grants;
mod host_state;
mod instance;
mod manager;
mod manifest;
mod package;
mod permission;
mod rate;
mod runtime;
mod slots;
mod storage;

pub mod api {
	pub use crate::bindings::oxidcord::plugin::types::*;
}

pub use broker::{HostBroker, NoopBroker};
pub use config::{HookTimeouts, RuntimeConfig};
pub use error::{Error, Result};
pub use fetch::{Fetcher, is_public};
pub use grants::{Grants, PluginGrant};
pub use instance::{CallError, PluginInstance, PluginState};
pub use manager::{CommandError, LoadIssue, PluginDebug, PluginInfo, PluginManager, SendDecision};
pub use manifest::{API_VERSION, MANIFEST_FILE, Manifest, PermissionRequest};
pub use package::{
	InstalledPlugin, MAX_PACKAGE_BYTES, PACKAGE_EXTENSION, Package, PackageFile, THEME_FILE, Trust,
};
pub use permission::Permission;
pub use runtime::Runtime;
pub use slots::{CommandEntry, SlotEntry, Slots};
pub use storage::{PluginStorage, STORAGE_QUOTA_BYTES};
