mod bindings;
mod broker;
pub mod catalog;
pub mod config;
pub mod conformance;
pub mod convert;
mod error;
mod fetch;
mod grants;
mod hook;
mod host_state;
mod instance;
mod latency;
mod manager;
mod manifest;
pub mod mounts;
mod package;
mod patches;
mod permission;
mod rate;
mod runtime;
mod slots;
mod storage;
mod timers;

pub mod api {
	pub use crate::bindings::oxidcord::plugin::types::*;

	pub mod model {
		pub use crate::bindings::ext::oxidcord::plugin::model::*;
	}
}

pub use broker::{HostBroker, NoopBroker};
pub use config::{HookTimeouts, RuntimeConfig};
pub use error::{Error, Result};
pub use fetch::{Fetcher, is_public};
pub use grants::{Grants, PluginGrant};
pub use hook::Hook;
pub use instance::{CallError, PluginInstance, PluginState};
pub use latency::LatencyStats;
pub use manager::{
	CommandError, EmojiPick, FullSendDecision, Intercepted, LoadIssue, PluginDebug, PluginInfo,
	PluginManager, RenderHooks, SendDecision,
};
pub use manifest::{API_VERSION, MANIFEST_FILE, Manifest, PermissionRequest, URL_SCHEMES};
pub use mounts::{Mount, Mounts, ViewContext};
pub use package::{
	InstalledPlugin, MAX_PACKAGE_BYTES, PACKAGE_EXTENSION, Package, PackageFile, THEME_FILE, Trust,
};
pub use patches::{Patches, PluginPatch};
pub use permission::{Permission, Tier};
pub use runtime::Runtime;
pub use slots::{CommandEntry, SlotEntry, Slots};
pub use storage::{PluginStorage, STORAGE_QUOTA_BYTES};
