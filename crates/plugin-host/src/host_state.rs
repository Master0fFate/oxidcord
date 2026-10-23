use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use store::Store;
use url::Url;
use wasmtime::ResourceLimiter;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::api::{
	ChannelSummary, FetchRequest, FetchResponse, GuildSummary, HostError, MemberSummary, MessageRef,
	MessageView, OutgoingMessage, UiTree, User, VoiceInfo,
};
use crate::bindings::oxidcord::plugin::{host, types};
use crate::broker::HostBroker;
use crate::config::RuntimeConfig;
use crate::convert::validate_ui_tree;
use crate::fetch::Fetcher;
use crate::grants::Grants;
use crate::instance::Health;
use crate::permission::Permission;
use crate::rate::RateLimits;
use crate::slots::{Commands, Slots};
use crate::storage::PluginStorage;

const MAX_MESSAGE_BYTES: usize = 4_000;
const MAX_LOG_BYTES: usize = 2_048;
const MAX_URL_BYTES: usize = 2_048;
const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;
const MAX_NOTIFY_TITLE_BYTES: usize = 128;
const MAX_NOTIFY_BODY_BYTES: usize = 1_024;
const MAX_EMOJI_BYTES: usize = 128;
const MAX_FETCHED_MESSAGES: u32 = 100;
const MAX_LISTED_MEMBERS: u32 = 1_000;

pub struct Services {
	pub store: Arc<Store>,
	pub grants: Arc<Grants>,
	pub broker: Arc<dyn HostBroker>,
	pub slots: Arc<Slots>,
	pub commands: Arc<Commands>,
	pub fetcher: Fetcher,
}

pub struct PluginLimits {
	memory_bytes: usize,
	table_elements: usize,
	pub memory_exceeded: bool,
}

impl ResourceLimiter for PluginLimits {
	fn memory_growing(
		&mut self,
		_current: usize,
		desired: usize,
		_maximum: Option<usize>,
	) -> wasmtime::Result<bool> {
		if desired > self.memory_bytes {
			self.memory_exceeded = true;
			wasmtime::bail!("plugin exceeded its memory limit");
		}
		Ok(true)
	}

	fn table_growing(
		&mut self,
		_current: usize,
		desired: usize,
		_maximum: Option<usize>,
	) -> wasmtime::Result<bool> {
		Ok(desired <= self.table_elements)
	}
}

pub struct HostState {
	plugin: Arc<str>,
	services: Arc<Services>,
	health: Option<Arc<Health>>,
	pub limits: PluginLimits,
	rate: RateLimits,
	storage: PluginStorage,
	wasi: WasiCtx,
	table: ResourceTable,
}

impl HostState {
	pub fn new(plugin: Arc<str>, services: Arc<Services>, config: &RuntimeConfig) -> Self {
		let storage = PluginStorage::new(Arc::clone(&services.store), &plugin);
		Self {
			plugin,
			services,
			health: None,
			limits: PluginLimits {
				memory_bytes: config.memory_limit_bytes,
				table_elements: config.table_elements,
				memory_exceeded: false,
			},
			rate: RateLimits::new(),
			storage,
			wasi: WasiCtxBuilder::new().build(),
			table: ResourceTable::new(),
		}
	}

	pub(crate) fn with_health(mut self, health: Arc<Health>) -> Self {
		self.health = Some(health);
		self
	}

	pub(crate) fn store_setting(&mut self, key: &str, value: &str) -> Result<(), HostError> {
		self.storage.set(&format!("settings.{key}"), value.as_bytes())
	}

	fn require(&self, permission: Permission) -> Result<(), HostError> {
		if self.services.grants.has(&self.plugin, permission) {
			Ok(())
		} else {
			if let Some(health) = &self.health {
				health.record_denied();
			}
			Err(HostError::Denied(permission.into()))
		}
	}

	fn has_private_access(&self) -> bool {
		self.services.grants.has(&self.plugin, Permission::ContextPrivate)
	}
}

impl WasiView for HostState {
	fn ctx(&mut self) -> WasiCtxView<'_> {
		WasiCtxView {
			ctx: &mut self.wasi,
			table: &mut self.table,
		}
	}
}

fn invalid(reason: &str) -> HostError {
	HostError::Invalid(reason.to_owned())
}

fn within_rate(allowed: bool) -> Result<(), HostError> {
	if allowed {
		Ok(())
	} else {
		Err(HostError::RateLimited)
	}
}

fn valid_name(name: &str, max: usize) -> bool {
	!name.is_empty()
		&& name.len() <= max
		&& name.bytes().all(|byte| {
			byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_' || byte == b'.'
		})
}

impl types::Host for HostState {}

impl host::Host for HostState {
	fn log(&mut self, level: u8, msg: String) {
		if !self.rate.log.take() {
			return;
		}
		let message = truncate(&msg, MAX_LOG_BYTES);
		let plugin = &*self.plugin;
		match level {
			0 => tracing::trace!(target: "plugin", plugin, "{message}"),
			1 => tracing::debug!(target: "plugin", plugin, "{message}"),
			2 => tracing::info!(target: "plugin", plugin, "{message}"),
			3 => tracing::warn!(target: "plugin", plugin, "{message}"),
			_ => tracing::error!(target: "plugin", plugin, "{message}"),
		}
	}

	fn storage_get(&mut self, key: String) -> Result<Option<Vec<u8>>, HostError> {
		self.require(Permission::Storage)?;
		self.storage.get(&key)
	}

	fn storage_set(&mut self, key: String, value: Vec<u8>) -> Result<(), HostError> {
		self.require(Permission::Storage)?;
		self.storage.set(&key, &value)
	}

	fn storage_delete(&mut self, key: String) -> Result<(), HostError> {
		self.require(Permission::Storage)?;
		self.storage.delete(&key)
	}

	fn storage_list(&mut self) -> Result<Vec<String>, HostError> {
		self.require(Permission::Storage)?;
		self.storage.list()
	}

	fn fetch(&mut self, req: FetchRequest) -> Result<FetchResponse, HostError> {
		self.require(Permission::NetFetch)?;
		within_rate(self.rate.fetch.take())?;
		let grants = &self.services.grants;
		let plugin = &self.plugin;
		self.services
			.fetcher
			.fetch(req, |host| grants.allows_host(plugin, host))
	}

	fn send_message(&mut self, msg: OutgoingMessage) -> Result<u64, HostError> {
		self.require(Permission::ActionsSend)?;
		if msg.content.is_empty() || msg.content.len() > MAX_MESSAGE_BYTES {
			return Err(invalid("message content length"));
		}
		within_rate(self.rate.send_message.take())?;
		self.services.broker.send_message(&self.plugin, msg)
	}

	fn add_reaction(&mut self, m: MessageRef, emoji: String) -> Result<(), HostError> {
		self.require(Permission::ActionsReact)?;
		if emoji.is_empty() || emoji.len() > MAX_EMOJI_BYTES {
			return Err(invalid("emoji length"));
		}
		within_rate(self.rate.add_reaction.take())?;
		self.services.broker.add_reaction(&self.plugin, m, emoji)
	}

	fn open_channel(&mut self, guild_id: Option<u64>, channel_id: u64) -> Result<(), HostError> {
		self.require(Permission::ActionsNavigate)?;
		self.services
			.broker
			.open_channel(&self.plugin, guild_id, channel_id)
	}

	fn clipboard_write(&mut self, text: String) -> Result<(), HostError> {
		self.require(Permission::ActionsClipboard)?;
		if text.len() > MAX_CLIPBOARD_BYTES {
			return Err(invalid("clipboard text too large"));
		}
		self.services.broker.clipboard_write(&self.plugin, text)
	}

	fn notify(&mut self, title: String, body: String) -> Result<(), HostError> {
		self.require(Permission::ActionsNotify)?;
		if title.len() > MAX_NOTIFY_TITLE_BYTES || body.len() > MAX_NOTIFY_BODY_BYTES {
			return Err(invalid("notification too long"));
		}
		within_rate(self.rate.notify.take())?;
		self.services.broker.notify(&self.plugin, title, body)
	}

	fn open_url(&mut self, url: String) -> Result<(), HostError> {
		self.require(Permission::ActionsOpenUrl)?;
		let parsed = Url::parse(&url).map_err(|_| invalid("invalid url"))?;
		if url.len() > MAX_URL_BYTES || !matches!(parsed.scheme(), "http" | "https") {
			return Err(invalid("url must be http or https"));
		}
		within_rate(self.rate.open_url.take())?;
		self.services.broker.open_url(&self.plugin, url)
	}

	fn current_user(&mut self) -> Result<User, HostError> {
		self.require(Permission::ContextBasic)?;
		self.services.broker.current_user(&self.plugin)
	}

	fn current_channel(&mut self) -> Result<Option<(Option<u64>, u64)>, HostError> {
		self.require(Permission::ContextBasic)?;
		let channel = self.services.broker.current_channel(&self.plugin)?;
		Ok(channel.filter(|(guild_id, _)| guild_id.is_some() || self.has_private_access()))
	}

	fn channel_messages(&mut self, channel_id: u64, limit: u32) -> Result<Vec<MessageView>, HostError> {
		self.require(Permission::ContextTimeline)?;
		let limit = limit.clamp(1, MAX_FETCHED_MESSAGES);
		let mut messages = self
			.services
			.broker
			.channel_messages(&self.plugin, channel_id, limit)?;
		if !self.has_private_access() {
			messages.retain(|message| message.guild_id.is_some());
		}
		messages.truncate(limit as usize);
		Ok(messages)
	}

	fn display_name(&mut self, guild_id: Option<u64>, user_id: u64) -> Result<String, HostError> {
		self.require(Permission::ContextMembers)?;
		self.services.broker.display_name(&self.plugin, guild_id, user_id)
	}

	fn list_guilds(&mut self) -> Result<Vec<GuildSummary>, HostError> {
		self.require(Permission::ContextDirectory)?;
		self.services.broker.list_guilds(&self.plugin)
	}

	fn list_channels(&mut self, guild_id: Option<u64>) -> Result<Vec<ChannelSummary>, HostError> {
		self.require(Permission::ContextDirectory)?;
		if guild_id.is_none() {
			self.require(Permission::ContextPrivate)?;
		}
		self.services.broker.list_channels(&self.plugin, guild_id)
	}

	fn list_members(&mut self, guild_id: u64, limit: u32) -> Result<Vec<MemberSummary>, HostError> {
		self.require(Permission::ContextDirectory)?;
		let limit = limit.clamp(1, MAX_LISTED_MEMBERS);
		let mut members = self.services.broker.list_members(&self.plugin, guild_id, limit)?;
		members.truncate(limit as usize);
		Ok(members)
	}

	fn voice_state(&mut self) -> Result<Option<VoiceInfo>, HostError> {
		self.require(Permission::ContextVoice)?;
		let info = self.services.broker.voice_state(&self.plugin)?;
		Ok(info.filter(|info| info.guild_id.is_some() || self.has_private_access()))
	}

	fn now_ms(&mut self) -> i64 {
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_or(0, |elapsed| elapsed.as_millis() as i64)
	}

	fn register_slot(&mut self, slot: String, node: UiTree) -> Result<(), HostError> {
		self.require(Permission::UiSlots)?;
		if !valid_name(&slot, 64) {
			return Err(invalid("invalid slot name"));
		}
		validate_ui_tree(&node).map_err(invalid)?;
		self.services.slots.set(&self.plugin, &slot, node.clone());
		self.services.broker.register_slot(&self.plugin, &slot, &node)
	}

	fn register_command(&mut self, name: String, description: String) -> Result<(), HostError> {
		self.require(Permission::UiSlots)?;
		if !valid_name(&name, 32) || description.len() > 256 {
			return Err(invalid("invalid command"));
		}
		if !self.services.commands.register(&self.plugin, &name, &description) {
			return Err(invalid("command is registered by another plugin"));
		}
		self.services
			.broker
			.register_command(&self.plugin, &name, &description)
	}
}

fn truncate(text: &str, max_bytes: usize) -> &str {
	if text.len() <= max_bytes {
		return text;
	}
	let mut end = max_bytes;
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	&text[..end]
}
