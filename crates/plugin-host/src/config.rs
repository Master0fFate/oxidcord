use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use semver::{Prerelease, Version};

#[derive(Debug, Clone, Copy)]
pub struct HookTimeouts {
	pub init: Duration,
	pub on_event: Duration,
	pub render_message: Duration,
	pub before_send: Duration,
	pub filter_message: Duration,
	pub display_user: Duration,
	pub ui_action: Duration,
	pub command: Duration,
	pub settings_schema: Duration,
}

impl Default for HookTimeouts {
	fn default() -> Self {
		Self {
			init: Duration::from_secs(5),
			on_event: Duration::from_millis(200),
			render_message: Duration::from_millis(50),
			before_send: Duration::from_millis(500),
			filter_message: Duration::from_millis(50),
			display_user: Duration::from_millis(50),
			ui_action: Duration::from_secs(2),
			command: Duration::from_secs(2),
			settings_schema: Duration::from_millis(500),
		}
	}
}

#[derive(Clone)]
pub struct RuntimeConfig {
	pub memory_limit_bytes: usize,
	pub table_elements: usize,
	pub max_wasm_stack: usize,
	pub timeouts: HookTimeouts,
	pub epoch_tick: Duration,
	pub caller_grace: Duration,
	pub event_queue_cap: usize,
	pub crash_limit: usize,
	pub crash_window: Duration,
	pub restart_backoff: Duration,
	pub max_restart_backoff: Duration,
	pub host_version: Version,
	pub catalog_key: Option<VerifyingKey>,
}

impl Default for RuntimeConfig {
	fn default() -> Self {
		Self {
			memory_limit_bytes: 64 * 1024 * 1024,
			table_elements: 10_000,
			max_wasm_stack: 512 * 1024,
			timeouts: HookTimeouts::default(),
			epoch_tick: Duration::from_millis(10),
			caller_grace: Duration::from_millis(100),
			event_queue_cap: 256,
			crash_limit: 5,
			crash_window: Duration::from_secs(60),
			restart_backoff: Duration::from_millis(100),
			max_restart_backoff: Duration::from_secs(5),
			host_version: Version::parse(env!("CARGO_PKG_VERSION")).map_or(
				Version::new(0, 1, 0),
				|mut version| {
					version.pre = Prerelease::EMPTY;
					version
				},
			),
			catalog_key: None,
		}
	}
}
