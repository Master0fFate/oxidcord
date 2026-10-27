use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use store::Store;
use url::Url;
use wasmtime::ResourceLimiter;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::api::model::{EmojiInfo, EmojiRef, MenuItem, MessageFull, MountOptions, StickerInfo, ViewTree};
use crate::api::{
	ChannelSummary, FetchRequest, FetchResponse, GuildSummary, HostError, MemberSummary, MessageRef,
	MessageView, OutgoingMessage, UiTree, User, VoiceInfo,
};
use crate::bindings::ext::oxidcord::plugin::{client, model, ui};
use crate::bindings::oxidcord::plugin::{host, types};
use crate::broker::HostBroker;
use crate::config::RuntimeConfig;
use crate::convert::{
	ImagePolicy, clip_line, sanitize_view, upgrade_ui_tree, valid_icon, valid_key, validate_ui_tree,
};
use crate::fetch::Fetcher;
use crate::grants::Grants;
use crate::instance::Health;
use crate::mounts::{self, MenuEntry, Mount, Mounts, Overlay, Provider};
use crate::patches::Patches;
use crate::permission::Permission;
use crate::rate::RateLimits;
use crate::slots::{Commands, Slots};
use crate::storage::PluginStorage;
use crate::timers::{self, Timers};

const MAX_MESSAGE_BYTES: usize = 4_000;
const MAX_LOG_BYTES: usize = 2_048;
const MAX_URL_BYTES: usize = 2_048;
const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;
const MAX_NOTIFY_TITLE_BYTES: usize = 128;
const MAX_NOTIFY_BODY_BYTES: usize = 1_024;
const MAX_EMOJI_BYTES: usize = 128;
const MAX_FETCHED_MESSAGES: u32 = 100;
const MAX_LISTED_MEMBERS: u32 = 1_000;
const MAX_SENDS_PER_GESTURE: u8 = 3;
const MAX_MENU_ITEMS: usize = 16;
const MAX_MENU_LABEL: usize = 64;
const MAX_MOUNT_TITLE: usize = 64;
const MAX_TIMER_TAG: usize = 64;
const MAX_COMPOSER_INSERT: usize = 2_000;
const MAX_MODAL_TITLE: usize = 96;
const TOAST_TTL_DEFAULT: Duration = Duration::from_secs(5);
const TOAST_TTL_MIN: Duration = Duration::from_millis(1_500);
const TOAST_TTL_MAX: Duration = Duration::from_secs(15);
const CDN: &str = "https://cdn.discordapp.com";
const MEDIA_PROXY: &str = "https://media.discordapp.net";

pub struct Services {
	pub store: Arc<Store>,
	pub grants: Arc<Grants>,
	pub broker: Arc<dyn HostBroker>,
	pub slots: Arc<Slots>,
	pub commands: Arc<Commands>,
	pub mounts: Arc<Mounts>,
	pub patches: Arc<Patches>,
	pub timers: Timers,
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
	gesture_sends: Option<u8>,
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
			gesture_sends: None,
			wasi: WasiCtxBuilder::new()
				.allow_tcp(false)
				.allow_udp(false)
				.allow_ip_name_lookup(false)
				.build(),
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
			Err(permission.denied())
		}
	}

	fn has_private_access(&self) -> bool {
		self.services.grants.has(&self.plugin, Permission::ContextPrivate)
	}

	pub(crate) fn begin_gesture(&mut self) {
		self.gesture_sends = Some(MAX_SENDS_PER_GESTURE);
	}

	pub(crate) fn end_gesture(&mut self) {
		self.gesture_sends = None;
	}

	fn in_gesture(&self) -> bool {
		self.gesture_sends.is_some()
	}

	fn take_gesture_send(&mut self) -> Result<(), HostError> {
		match &mut self.gesture_sends {
			Some(left) if *left > 0 => {
				*left -= 1;
				Ok(())
			}
			_ => Err(invalid("requires a user action")),
		}
	}

	fn image_policy(&self) -> ImagePolicy {
		ImagePolicy::new(false, self.services.grants.get(&self.plugin).image_hosts)
	}

	fn view(&self, tree: &ViewTree) -> ViewTree {
		sanitize_view(tree, &self.image_policy())
	}

	fn changed(&self) {
		self.services.broker.surfaces_changed();
	}

	fn check_outgoing(msg: &OutgoingMessage) -> Result<(), HostError> {
		if msg.content.is_empty() || msg.content.len() > MAX_MESSAGE_BYTES {
			return Err(invalid("message content length"));
		}
		Ok(())
	}

	fn visible(&self, message: &MessageFull) -> bool {
		message.view.guild_id.is_some() || self.has_private_access()
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
		Self::check_outgoing(&msg)?;
		self.take_gesture_send()?;
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
		let tree = sanitize_view(&upgrade_ui_tree(&node), &ImagePolicy::new(true, BTreeSet::new()));
		self.services.slots.set(&self.plugin, &slot, tree);
		self.changed();
		Ok(())
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

fn mount_options(options: MountOptions) -> MountOptions {
	MountOptions {
		title: clip_line(&options.title, MAX_MOUNT_TITLE),
		icon: options.icon.filter(|icon| valid_icon(icon)),
		order: options.order,
	}
}

fn menu_items(items: Vec<MenuItem>) -> Vec<MenuItem> {
	let mut kept: Vec<MenuItem> = Vec::new();
	for item in items.into_iter().take(MAX_MENU_ITEMS) {
		let label = clip_line(&item.label, MAX_MENU_LABEL);
		if !valid_key(&item.key) || label.trim().is_empty() || kept.iter().any(|other| other.key == item.key)
		{
			continue;
		}
		kept.push(MenuItem {
			label,
			icon: item.icon.filter(|icon| valid_icon(icon)),
			..item
		});
	}
	let roots: Vec<String> = kept
		.iter()
		.filter(|item| item.parent.is_none())
		.map(|item| item.key.clone())
		.collect();
	kept.retain(|item| item.parent.as_ref().is_none_or(|parent| roots.contains(parent)));
	kept
}

impl model::Host for HostState {}

impl ui::Host for HostState {
	fn mount(
		&mut self,
		point: String,
		key: String,
		tree: ViewTree,
		options: MountOptions,
	) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		let point = mounts::static_point(&point).ok_or_else(|| invalid("unknown mount point"))?;
		if !valid_key(&key) {
			return Err(invalid("invalid mount key"));
		}
		within_rate(self.rate.ui.take())?;
		let mount = Mount {
			plugin_id: Arc::clone(&self.plugin),
			key,
			tree: self.view(&tree),
			options: mount_options(options),
		};
		self.services
			.mounts
			.mount(point, mount)
			.map_err(|_| invalid("too many mounts"))?;
		self.changed();
		Ok(())
	}

	fn update(&mut self, point: String, key: String, tree: ViewTree) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		let point = mounts::static_point(&point).ok_or_else(|| invalid("unknown mount point"))?;
		within_rate(self.rate.ui.take())?;
		let tree = self.view(&tree);
		self.services
			.mounts
			.update(point, &self.plugin, &key, tree)
			.map_err(|_| invalid("nothing is mounted under that key"))?;
		self.changed();
		Ok(())
	}

	fn unmount(&mut self, point: String, key: String) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		let point = mounts::static_point(&point).ok_or_else(|| invalid("unknown mount point"))?;
		self.services.mounts.unmount(point, &self.plugin, &key);
		self.changed();
		Ok(())
	}

	fn provide(&mut self, point: String, options: MountOptions) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		let point = mounts::context_point(&point).ok_or_else(|| invalid("unknown contextual point"))?;
		self.services.mounts.provide(
			point,
			Provider {
				plugin_id: Arc::clone(&self.plugin),
				options: mount_options(options),
			},
		);
		self.changed();
		Ok(())
	}

	fn invalidate(&mut self, point: String) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		let point = mounts::context_point(&point).ok_or_else(|| invalid("unknown contextual point"))?;
		within_rate(self.rate.ui.take())?;
		self.services.mounts.invalidate(&self.plugin, point);
		self.changed();
		Ok(())
	}

	fn set_menu_items(&mut self, menu: String, items: Vec<MenuItem>) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		let menu = mounts::menu(&menu).ok_or_else(|| invalid("unknown menu"))?;
		self.services.mounts.set_menu(
			menu,
			MenuEntry {
				plugin_id: Arc::clone(&self.plugin),
				items: menu_items(items),
			},
		);
		self.changed();
		Ok(())
	}

	fn toast(&mut self, tree: ViewTree, ttl_ms: u32) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		if !self.in_gesture() {
			within_rate(self.rate.toast.take())?;
		}
		let ttl = match ttl_ms {
			0 => TOAST_TTL_DEFAULT,
			ms => Duration::from_millis(u64::from(ms)).clamp(TOAST_TTL_MIN, TOAST_TTL_MAX),
		};
		let tree = self.view(&tree);
		if tree.roots.is_empty() {
			return Err(invalid("empty toast"));
		}
		self.services.mounts.push_overlay(Overlay::Toast {
			plugin_id: Arc::clone(&self.plugin),
			tree,
			ttl,
		});
		self.changed();
		Ok(())
	}

	fn open_modal(&mut self, title: String, tree: ViewTree) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		if !self.in_gesture() {
			return Err(invalid("requires a user action"));
		}
		let tree = self.view(&tree);
		self.services.mounts.push_overlay(Overlay::Modal {
			plugin_id: Arc::clone(&self.plugin),
			title: Some(clip_line(&title, MAX_MODAL_TITLE)),
			tree,
		});
		self.changed();
		Ok(())
	}
}

fn cdn_size(size: u32) -> u32 {
	size.clamp(16, 512)
}

impl client::Host for HostState {
	fn set_timer(&mut self, delay_ms: u32, repeat_ms: u32, tag: String) -> Result<u64, HostError> {
		within_rate(self.rate.timer.take())?;
		let delay = Duration::from_millis(u64::from(delay_ms)).max(timers::MIN_DELAY);
		let repeat =
			(repeat_ms > 0).then(|| Duration::from_millis(u64::from(repeat_ms)).max(timers::MIN_REPEAT));
		self.services
			.timers
			.set(&self.plugin, delay, repeat, clip_line(&tag, MAX_TIMER_TAG))
			.map_err(|_| invalid("too many timers"))
	}

	fn clear_timer(&mut self, id: u64) {
		self.services.timers.clear(&self.plugin, id);
	}

	fn list_emojis(&mut self, channel_id: Option<u64>) -> Result<Vec<EmojiInfo>, HostError> {
		self.require(Permission::ContextExpressions)?;
		self.services.broker.list_emojis(&self.plugin, channel_id)
	}

	fn emoji_url(&mut self, emoji: EmojiRef, size: u32) -> Option<String> {
		let id = emoji.id?;
		let extension = if emoji.animated { "gif" } else { "webp" };
		Some(format!("{CDN}/emojis/{id}.{extension}?size={}", cdn_size(size)))
	}

	fn list_stickers(&mut self, guild_id: Option<u64>) -> Result<Vec<StickerInfo>, HostError> {
		self.require(Permission::ContextExpressions)?;
		self.services.broker.list_stickers(&self.plugin, guild_id)
	}

	fn sticker_url(&mut self, sticker: StickerInfo, size: u32) -> String {
		let id = sticker.id;
		let size = cdn_size(size);
		match sticker.format {
			4 => format!("{MEDIA_PROXY}/stickers/{id}.gif?size={size}"),
			3 => format!("{CDN}/stickers/{id}.json"),
			_ => format!("{CDN}/stickers/{id}.png?size={size}"),
		}
	}

	fn composer_insert(&mut self, text: String) -> Result<(), HostError> {
		self.require(Permission::Composer)?;
		if text.is_empty() || text.len() > MAX_COMPOSER_INSERT {
			return Err(invalid("text length"));
		}
		within_rate(self.rate.composer.take())?;
		self.services.mounts.push_overlay(Overlay::Insert {
			plugin_id: Arc::clone(&self.plugin),
			text,
		});
		self.changed();
		Ok(())
	}

	fn get_message(&mut self, channel_id: u64, message_id: u64) -> Result<Option<MessageFull>, HostError> {
		self.require(Permission::ContextTimeline)?;
		let message = self
			.services
			.broker
			.message_full(&self.plugin, channel_id, message_id)?;
		Ok(message.filter(|message| self.visible(message)))
	}

	fn channel_messages_full(&mut self, channel_id: u64, limit: u32) -> Result<Vec<MessageFull>, HostError> {
		self.require(Permission::ContextTimeline)?;
		let limit = limit.clamp(1, MAX_FETCHED_MESSAGES);
		let mut messages = self
			.services
			.broker
			.channel_messages_full(&self.plugin, channel_id, limit)?;
		messages.retain(|message| self.visible(message));
		messages.truncate(limit as usize);
		Ok(messages)
	}

	fn send_as_user(&mut self, msg: OutgoingMessage) -> Result<(), HostError> {
		self.require(Permission::ActionsSendUser)?;
		Self::check_outgoing(&msg)?;
		self.take_gesture_send()?;
		within_rate(self.rate.send_message.take())?;
		self.services.broker.send_as_user(&self.plugin, msg)
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
