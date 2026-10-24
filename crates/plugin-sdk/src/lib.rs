#[cfg(not(feature = "ext"))]
#[allow(
	clippy::all,
	clippy::pedantic,
	unused_imports,
	dead_code,
	unsafe_op_in_unsafe_fn
)]
pub mod bindings {
	wit_bindgen::generate!({
		path: "../plugin-host/wit/oxidcord.wit",
		world: "plugin",
		pub_export_macro: true,
		export_macro_name: "export_guest",
		default_bindings_module: "oxidcord_plugin_sdk::bindings",
		additional_derives: [PartialEq],
	});
}

#[cfg(feature = "ext")]
#[allow(
	clippy::all,
	clippy::pedantic,
	unused_imports,
	dead_code,
	unsafe_op_in_unsafe_fn
)]
pub mod bindings {
	wit_bindgen::generate!({
		path: "../plugin-host/wit/oxidcord.wit",
		world: "plugin-ext",
		pub_export_macro: true,
		export_macro_name: "export_guest",
		default_bindings_module: "oxidcord_plugin_sdk::bindings",
		additional_derives: [PartialEq],
	});
}

pub mod host {
	pub use crate::bindings::oxidcord::plugin::host::*;
}

#[cfg(feature = "ext")]
pub mod client;
#[cfg(feature = "ext")]
mod ext;
pub mod log;
pub mod md;
#[cfg(feature = "ext")]
pub mod menu;
#[cfg(feature = "ext")]
pub mod mount;
#[cfg(feature = "ext")]
pub mod patch;
mod plugin;
pub mod storage;
#[cfg(feature = "ext")]
pub mod timer;
pub mod ui;
#[cfg(feature = "ext")]
pub mod view;

pub mod types {
	pub use crate::bindings::oxidcord::plugin::types::*;
}

#[cfg(feature = "ext")]
pub mod model {
	pub use crate::bindings::oxidcord::plugin::model::*;
}

#[cfg(feature = "ext")]
pub use ext::{__private as __private_ext, PluginExt, Reply};
pub use md::{Color, Md};
pub use plugin::{__private, Plugin};
pub use types::{
	ChannelSummary, DisplayOverride, Event, GuildSummary, HostError, MemberSummary, MessageRef, MessageView,
	OutgoingMessage, Permission, SendDecision, User, VoiceInfo,
};
pub use ui::Ui;

pub const DELETED_FLAG: u64 = 1 << 63;

pub fn now_ms() -> i64 {
	host::now_ms()
}

pub fn is_deleted(msg: &MessageView) -> bool {
	msg.flags & DELETED_FLAG != 0
}
