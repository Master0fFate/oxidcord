use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use protocol::StatusKind;
use store::Store;
use url::Url;
use wasmtime::ResourceLimiter;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::api::model::{
	AuthSession, ChannelDetail, ClickWatch, ClientInfo, ClientOption, ContextDetail, DmSection, EmojiInfo,
	EmojiRef, FileBlob, Folder, GuildDetail, Keybind, MemberInfo, MenuItem, MenuItemExt, MenuWhenExt,
	MessageFull, MessageMeta, MountOptions, NotificationSettings, PickedFile, PluginSummary, Presence,
	ReadState, Relationship, RoleInfo, StickerInfo, UserProfile, ViewTree, VoiceDetail,
};
use crate::api::{
	ChannelSummary, FetchRequest, FetchResponse, GuildSummary, HostError, MemberSummary, MessageRef,
	MessageView, OutgoingMessage, UiTree, User, VoiceInfo,
};
use crate::bindings::ext::oxidcord::plugin::{client, model, ui};
use crate::bindings::oxidcord::plugin::{host, types};
use crate::broker::{HostBroker, Invalidation, Navigation};
use crate::config::RuntimeConfig;
use crate::convert::{
	ImagePolicy, clip_line, sanitize_view, upgrade_ui_tree, valid_icon, valid_key, validate_ui_tree,
};
use crate::fetch::Fetcher;
use crate::grants::Grants;
use crate::instance::Health;
use crate::keybinds::Keybinds;
use crate::latency::LatencyStats;
use crate::mounts::{self, MenuEntry, Mount, Mounts, Overlay, Provider};
use crate::patches::Patches;
use crate::permission::Permission;
use crate::rate::RateLimits;
use crate::slots::{Commands, Slots};
use crate::storage::PluginStorage;
use crate::timers::{self, Timers};
use crate::watches::Watches;

const MAX_MESSAGE_BYTES: usize = 4_000;
const MAX_LOG_BYTES: usize = 2_048;
const MAX_URL_BYTES: usize = 2_048;
const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;
const MAX_NOTIFY_TITLE_BYTES: usize = 128;
const MAX_NOTIFY_BODY_BYTES: usize = 1_024;
const MAX_EMOJI_BYTES: usize = 128;
const MAX_FETCHED_MESSAGES: u32 = 100;
const MAX_LISTED_MEMBERS: u32 = 1_000;
const MAX_LISTED_EMOJIS: u32 = 10_000;
const MAX_REACTION_USERS: u32 = 100;
const MAX_SPOKEN_CHARS: usize = 300;
const MAX_SENDS_PER_GESTURE: u8 = 3;
const MAX_CUSTOM_STATUS_CHARS: usize = 128;
const MAX_CUSTOM_STATUS_BYTES: usize = 512;
const MAX_PICKED_HANDLES: usize = 16;
const MAX_ATTACHED_FILES: usize = 10;
const MAX_UPLOAD_FILES: usize = 4;
const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_FILE_NAME_BYTES: usize = 255;
const MAX_DM_SECTIONS: usize = 16;
const MAX_DM_CHANNELS: usize = 200;
const MAX_DM_SECTION_TITLE: usize = 64;
const MIN_EMOJI_NAME: usize = 2;
const MAX_EMOJI_NAME: usize = 32;
const MAX_NOTIFICATION_LEVEL: u8 = 3;
const MAX_NOTIFICATION_VOLUME: u8 = 100;
const MAX_FORUM_LAYOUT: u8 = 2;
const MAX_FORUM_SORT: u8 = 1;
pub const SETTINGS_PAGES: [&str; 20] = [
	"my-account",
	"profiles",
	"privacy",
	"devices",
	"connections",
	"clips",
	"friend-requests",
	"appearance",
	"accessibility",
	"voice-video",
	"chat",
	"notifications",
	"keybinds",
	"language",
	"streamer-mode",
	"advanced",
	"plugins",
	"themes",
	"developer",
	"updates",
];
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallContext {
	Init,
	Event,
	Timer,
	Render,
	Intercept,
	Display,
	Decorate,
	Autocomplete,
	Notification,
	UserAction,
}

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
	pub latency: LatencyStats,
	pub keybinds: Arc<Keybinds>,
	pub watches: Arc<Watches>,
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
	context: CallContext,
	detail: Option<ContextDetail>,
	picked: BTreeMap<u64, PathBuf>,
	next_pick: u64,
	blocked_mark: Duration,
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
			context: CallContext::Init,
			detail: None,
			picked: BTreeMap::new(),
			next_pick: 1,
			blocked_mark: Duration::ZERO,
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

	pub(crate) fn set_context(&mut self, context: CallContext) {
		self.context = context;
	}

	pub(crate) fn set_detail(&mut self, detail: Option<ContextDetail>) {
		self.detail = detail;
	}

	fn blocked_total(&self) -> Duration {
		self.health
			.as_ref()
			.map_or(Duration::ZERO, |health| health.blocked_for())
	}

	pub(crate) fn mark_blocked_time(&mut self) {
		self.blocked_mark = self.blocked_total();
	}

	pub(crate) fn take_blocked_time(&mut self) -> Duration {
		let total = self.blocked_total();
		let fresh = total.saturating_sub(self.blocked_mark);
		self.blocked_mark = total;
		fresh
	}

	fn blocking<T>(&self, run: impl FnOnce(&dyn HostBroker) -> T) -> T {
		if let Some(health) = &self.health {
			health.begin_blocking();
		}
		let result = run(self.services.broker.as_ref());
		if let Some(health) = &self.health {
			health.end_blocking();
		}
		result
	}

	fn loads(&self) -> Result<(), HostError> {
		match self.context {
			CallContext::Event | CallContext::Timer | CallContext::UserAction => Ok(()),
			_ => Err(HostError::Unavailable),
		}
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

	fn require_gesture(&self) -> Result<(), HostError> {
		if self.in_gesture() {
			Ok(())
		} else {
			Err(invalid("requires a user action"))
		}
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

	fn view(&self, tree: &ViewTree) -> ViewTree {
		sanitize_view(tree, &self.services.grants.image_policy(&self.plugin))
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
		let fetcher = &self.services.fetcher;
		self.blocking(|_| fetcher.fetch(req, |host| grants.allows_host(plugin, host)))
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
		if url.len() > MAX_URL_BYTES {
			return Err(invalid("url is too long"));
		}
		if !matches!(parsed.scheme(), "http" | "https") {
			if !self
				.services
				.grants
				.allows_url_scheme(&self.plugin, parsed.scheme())
			{
				return Err(invalid("url scheme is not approved"));
			}
			self.require_gesture()?;
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
		epoch_millis()
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
			tree: Arc::new(self.view(&tree)),
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
				items: menu_items(items).into(),
				extra: Arc::from([]),
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
		self.require_gesture()?;
		let tree = self.view(&tree);
		self.services.mounts.push_overlay(Overlay::Modal {
			plugin_id: Arc::clone(&self.plugin),
			title: Some(clip_line(&title, MAX_MODAL_TITLE)),
			tree,
		});
		self.changed();
		Ok(())
	}

	fn set_menu_items_ext(&mut self, menu: String, items: Vec<MenuItemExt>) -> Result<(), HostError> {
		self.require(Permission::UiMount)?;
		let menu = mounts::menu(&menu).ok_or_else(|| invalid("unknown menu"))?;
		let flags: Vec<(String, MenuWhenExt)> = items
			.iter()
			.map(|item| (item.base.key.clone(), item.extra))
			.collect();
		let kept = menu_items(items.into_iter().map(|item| item.base).collect());
		let extra: Vec<(String, MenuWhenExt)> = flags
			.into_iter()
			.filter(|(key, extra)| *extra != MenuWhenExt::empty() && kept.iter().any(|item| item.key == *key))
			.collect();
		self.services.mounts.set_menu(
			menu,
			MenuEntry {
				plugin_id: Arc::clone(&self.plugin),
				items: kept.into(),
				extra: extra.into(),
			},
		);
		self.changed();
		Ok(())
	}

	fn context_detail(&mut self) -> Option<ContextDetail> {
		if self.services.grants.has(&self.plugin, Permission::UiMount) {
			self.detail.clone()
		} else {
			None
		}
	}

	fn watch_clicks(&mut self, watch: ClickWatch) -> Result<(), HostError> {
		self.require(Permission::UiInput)?;
		within_rate(self.rate.ui.take())?;
		self.services.watches.watch_messages(&self.plugin, watch);
		Ok(())
	}

	fn watch_channel_clicks(&mut self, watch: ClickWatch) -> Result<(), HostError> {
		self.require(Permission::UiInput)?;
		within_rate(self.rate.ui.take())?;
		self.services.watches.watch_channels(&self.plugin, watch);
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
		self.services
			.broker
			.list_emojis(&self.plugin, channel_id, MAX_LISTED_EMOJIS)
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

	fn list_relationships(&mut self) -> Result<Vec<Relationship>, HostError> {
		self.require(Permission::ContextRelationships)?;
		self.services.broker.relationships(&self.plugin)
	}

	fn user_presence(&mut self, user_id: u64) -> Result<Option<Presence>, HostError> {
		self.require(Permission::ContextPresence)?;
		self.services.broker.presence(&self.plugin, user_id)
	}

	fn idle_seconds(&mut self) -> Result<u32, HostError> {
		self.require(Permission::ContextPresence)?;
		self.services.broker.idle_seconds(&self.plugin)
	}

	fn read_state(&mut self, channel_id: u64) -> Result<Option<ReadState>, HostError> {
		self.require(Permission::ContextReadState)?;
		self.services
			.broker
			.read_state(&self.plugin, channel_id, self.has_private_access())
	}

	fn unread_channels(&mut self, guild_id: Option<u64>) -> Result<Vec<ReadState>, HostError> {
		self.require(Permission::ContextReadState)?;
		self.services
			.broker
			.unread_channels(&self.plugin, guild_id, self.has_private_access())
	}

	fn channel_muted(&mut self, channel_id: u64) -> Result<bool, HostError> {
		self.require(Permission::ContextReadState)?;
		self.services
			.broker
			.channel_muted(&self.plugin, channel_id, self.has_private_access())
	}

	fn list_roles(&mut self, guild_id: u64) -> Result<Vec<RoleInfo>, HostError> {
		self.require(Permission::ContextRoles)?;
		self.services.broker.roles(&self.plugin, guild_id)
	}

	fn member_info(&mut self, guild_id: u64, user_id: u64) -> Result<Option<MemberInfo>, HostError> {
		self.require(Permission::ContextRoles)?;
		self.services.broker.member_info(&self.plugin, guild_id, user_id)
	}

	fn channel_permissions(&mut self, channel_id: u64, user_id: Option<u64>) -> Result<u64, HostError> {
		self.require(Permission::ContextRoles)?;
		self.services
			.broker
			.channel_permissions(&self.plugin, channel_id, user_id, self.has_private_access())
	}

	fn guild_detail(&mut self, guild_id: u64) -> Result<Option<GuildDetail>, HostError> {
		self.require(Permission::ContextDirectory)?;
		self.services.broker.guild_detail(&self.plugin, guild_id)
	}

	fn channel_detail(&mut self, channel_id: u64) -> Result<Option<ChannelDetail>, HostError> {
		self.require(Permission::ContextDirectory)?;
		self.services
			.broker
			.channel_detail(&self.plugin, channel_id, self.has_private_access())
	}

	fn list_folders(&mut self) -> Result<Vec<Folder>, HostError> {
		self.require(Permission::ContextDirectory)?;
		self.services.broker.folders(&self.plugin)
	}

	fn user_profile(&mut self, user_id: u64) -> Result<Option<UserProfile>, HostError> {
		self.require(Permission::ContextProfiles)?;
		if let Some(profile) = self.services.broker.cached_profile(&self.plugin, user_id) {
			return Ok(Some(profile));
		}
		self.loads()?;
		within_rate(self.rate.lookup.take())?;
		self.blocking(|broker| broker.load_profile(&self.plugin, user_id))
	}

	fn list_sessions(&mut self) -> Result<Vec<AuthSession>, HostError> {
		self.require(Permission::ContextSessions)?;
		self.services.broker.sessions(&self.plugin)
	}

	fn client_info(&mut self) -> Result<ClientInfo, HostError> {
		self.require(Permission::ContextBasic)?;
		self.services.broker.client_info(&self.plugin)
	}

	fn voice_detail(&mut self) -> Result<Option<VoiceDetail>, HostError> {
		self.require(Permission::ContextVoice)?;
		let detail = self
			.services
			.broker
			.voice_detail(&self.plugin, self.has_private_access())?;
		Ok(detail.filter(|detail| detail.base.guild_id.is_some() || self.has_private_access()))
	}

	fn fetch_message(&mut self, channel_id: u64, message_id: u64) -> Result<Option<MessageFull>, HostError> {
		self.require(Permission::ContextTimeline)?;
		let private = self.has_private_access();
		let broker = &self.services.broker;
		if let Some(message) = broker.cached_message(&self.plugin, channel_id, message_id) {
			return Ok(Some(message).filter(|message| self.visible(message)));
		}
		self.loads()?;
		within_rate(self.rate.fetch_message.take())?;
		let message =
			self.blocking(|broker| broker.load_message(&self.plugin, channel_id, message_id, private))?;
		Ok(message.filter(|message| self.visible(message)))
	}

	fn message_meta(&mut self, m: MessageRef) -> Result<Option<MessageMeta>, HostError> {
		self.require(Permission::ContextTimeline)?;
		self.services
			.broker
			.message_meta(&self.plugin, &m, self.has_private_access())
	}

	fn reaction_users(&mut self, m: MessageRef, emoji: EmojiRef, limit: u32) -> Result<Vec<User>, HostError> {
		self.require(Permission::ContextTimeline)?;
		if emoji.name.is_empty() || emoji.name.len() > MAX_EMOJI_BYTES {
			return Err(invalid("emoji length"));
		}
		let limit = limit.clamp(1, MAX_REACTION_USERS);
		let private = self.has_private_access();
		let broker = &self.services.broker;
		if let Some(users) = broker.cached_reaction_users(&self.plugin, &m, &emoji, limit, private) {
			return Ok(users);
		}
		self.loads()?;
		within_rate(self.rate.reaction_users.take())?;
		self.blocking(|broker| broker.load_reaction_users(&self.plugin, &m, &emoji, limit, private))
	}

	fn fetch_user(&mut self, user_id: u64) -> Result<Option<User>, HostError> {
		self.require(Permission::ContextMembers)?;
		if let Some(user) = self.services.broker.cached_user(&self.plugin, user_id) {
			return Ok(Some(user));
		}
		self.loads()?;
		within_rate(self.rate.lookup.take())?;
		self.blocking(|broker| broker.load_user(&self.plugin, user_id))
	}

	fn attachment_bytes(&mut self, m: MessageRef, attachment_id: u64) -> Result<Vec<u8>, HostError> {
		self.require(Permission::ContextAttachments)?;
		self.loads()?;
		within_rate(self.rate.attachment_bytes.take())?;
		let private = self.has_private_access();
		self.blocking(|broker| broker.attachment_bytes(&self.plugin, &m, attachment_id, private))
	}

	fn avatar_bytes(&mut self, user_id: u64, size: u32) -> Result<Vec<u8>, HostError> {
		self.require(Permission::ContextAttachments)?;
		self.loads()?;
		within_rate(self.rate.attachment_bytes.take())?;
		self.blocking(|broker| broker.avatar_bytes(&self.plugin, user_id, cdn_size(size)))
	}

	fn list_favorite_emojis(&mut self) -> Result<Vec<EmojiRef>, HostError> {
		self.require(Permission::ContextExpressions)?;
		self.services.broker.favorite_emojis(&self.plugin)
	}

	fn list_plugins(&mut self) -> Result<Vec<PluginSummary>, HostError> {
		self.require(Permission::PluginsList)?;
		self.services.broker.plugins(&self.plugin)
	}

	fn composer_get_draft(&mut self) -> Result<String, HostError> {
		self.require(Permission::Composer)?;
		self.services
			.broker
			.composer_draft(&self.plugin, self.has_private_access())
	}

	fn set_status(&mut self, status: String) -> Result<(), HostError> {
		self.require(Permission::ActionsPresence)?;
		let status = StatusKind::from(status.as_str());
		if !matches!(
			status,
			StatusKind::Online | StatusKind::Idle | StatusKind::Dnd | StatusKind::Invisible
		) {
			return Err(invalid("unknown status"));
		}
		within_rate(self.rate.presence.take())?;
		self.services.broker.set_status(&self.plugin, status)
	}

	fn set_custom_status(
		&mut self,
		text: String,
		emoji: Option<EmojiRef>,
		expires_ms: Option<i64>,
	) -> Result<(), HostError> {
		self.require(Permission::ActionsPresence)?;
		if text.chars().count() > MAX_CUSTOM_STATUS_CHARS {
			return Err(invalid("custom status text length"));
		}
		if emoji.as_ref().is_some_and(|emoji| !valid_emoji(emoji)) {
			return Err(invalid("emoji length"));
		}
		if expires_ms.is_some_and(|expires| expires <= epoch_millis()) {
			return Err(invalid("expiry is in the past"));
		}
		within_rate(self.rate.presence.take())?;
		self.services.broker.set_custom_status(
			&self.plugin,
			clip_line(&text, MAX_CUSTOM_STATUS_BYTES),
			emoji,
			expires_ms,
		)
	}

	fn mark_read(&mut self, channel_id: u64) -> Result<(), HostError> {
		self.require(Permission::ActionsAck)?;
		within_rate(self.rate.ack.take())?;
		self.services
			.broker
			.mark_read(&self.plugin, channel_id, self.has_private_access())
	}

	fn mark_unread(&mut self, m: MessageRef) -> Result<(), HostError> {
		self.require(Permission::ActionsAck)?;
		within_rate(self.rate.ack.take())?;
		self.services
			.broker
			.mark_unread(&self.plugin, &m, self.has_private_access())
	}

	fn mark_guild_read(&mut self, guild_id: u64) -> Result<(), HostError> {
		self.require(Permission::ActionsAck)?;
		self.require_gesture()?;
		within_rate(self.rate.ack.take())?;
		self.services.broker.mark_guild_read(&self.plugin, guild_id)
	}

	fn start_reply(&mut self, m: MessageRef, mention: bool) -> Result<(), HostError> {
		self.require(Permission::ActionsCompose)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.start_reply(&self.plugin, &m, mention, self.has_private_access())
	}

	fn start_edit(&mut self, m: MessageRef) -> Result<(), HostError> {
		self.require(Permission::ActionsCompose)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.start_edit(&self.plugin, &m, self.has_private_access())
	}

	fn jump_to_message(&mut self, m: MessageRef) -> Result<(), HostError> {
		self.require(Permission::ActionsCompose)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.jump_to_message(&self.plugin, &m, self.has_private_access())
	}

	fn jump_to_top(&mut self, channel_id: u64) -> Result<(), HostError> {
		self.require(Permission::ActionsCompose)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.jump_to_top(&self.plugin, channel_id, self.has_private_access())
	}

	fn edit_message(&mut self, m: MessageRef, content: String) -> Result<(), HostError> {
		self.require(Permission::ActionsEdit)?;
		self.require_gesture()?;
		if content.is_empty() || content.len() > MAX_MESSAGE_BYTES {
			return Err(invalid("message content length"));
		}
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.edit_message(&self.plugin, &m, content, self.has_private_access())
	}

	fn delete_message(&mut self, m: MessageRef) -> Result<(), HostError> {
		self.require(Permission::ActionsDelete)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.delete_message(&self.plugin, &m, self.has_private_access())
	}

	fn react(&mut self, m: MessageRef, emoji: EmojiRef, burst: bool) -> Result<(), HostError> {
		self.require(Permission::ActionsReact)?;
		if burst {
			self.require_gesture()?;
		}
		if !valid_emoji(&emoji) {
			return Err(invalid("emoji length"));
		}
		within_rate(self.rate.add_reaction.take())?;
		self.services
			.broker
			.react(&self.plugin, &m, &emoji, burst, self.has_private_access())
	}

	fn remove_reaction(&mut self, m: MessageRef, emoji: EmojiRef) -> Result<(), HostError> {
		self.require(Permission::ActionsReact)?;
		if !valid_emoji(&emoji) {
			return Err(invalid("emoji length"));
		}
		within_rate(self.rate.add_reaction.take())?;
		self.services
			.broker
			.remove_reaction(&self.plugin, &m, &emoji, self.has_private_access())
	}

	fn pick_files(&mut self, multiple: bool) -> Result<Vec<PickedFile>, HostError> {
		self.require(Permission::ActionsUpload)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		let picked = self.blocking(|broker| broker.pick_files(&self.plugin, multiple));
		let limit = if multiple { MAX_PICKED_HANDLES } else { 1 };
		Ok(picked?
			.into_iter()
			.take(limit)
			.map(|file| PickedFile {
				handle: self.remember_pick(file.path),
				name: clip_line(&file.name, MAX_FILE_NAME_BYTES),
				size: file.size,
				content_type: file.content_type,
			})
			.collect())
	}

	fn composer_attach_files(&mut self, handles: Vec<u64>) -> Result<(), HostError> {
		self.require(Permission::ActionsUpload)?;
		self.require_gesture()?;
		if handles.is_empty() || handles.len() > MAX_ATTACHED_FILES {
			return Err(invalid("file count"));
		}
		let paths = handles
			.iter()
			.map(|handle| {
				self.picked
					.get(handle)
					.cloned()
					.ok_or_else(|| invalid("unknown file handle"))
			})
			.collect::<Result<Vec<PathBuf>, HostError>>()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.attach_files(&self.plugin, paths, self.has_private_access())
	}

	fn send_message_files(&mut self, msg: OutgoingMessage, files: Vec<FileBlob>) -> Result<(), HostError> {
		self.require(Permission::ActionsUpload)?;
		self.require_gesture()?;
		if msg.content.len() > MAX_MESSAGE_BYTES {
			return Err(invalid("message content length"));
		}
		check_blobs(&files)?;
		self.take_gesture_send()?;
		within_rate(self.rate.send_message.take())?;
		self.services.broker.send_message_files(&self.plugin, msg, files)
	}

	fn composer_set_draft(&mut self, text: String) -> Result<(), HostError> {
		self.require(Permission::Composer)?;
		self.require_gesture()?;
		if text.len() > MAX_MESSAGE_BYTES {
			return Err(invalid("text length"));
		}
		within_rate(self.rate.composer.take())?;
		self.services
			.broker
			.set_draft(&self.plugin, text, self.has_private_access())
	}

	fn composer_attach_sticker(&mut self, sticker_id: u64) -> Result<(), HostError> {
		self.require(Permission::Composer)?;
		self.require_gesture()?;
		within_rate(self.rate.composer.take())?;
		self.services
			.broker
			.attach_sticker(&self.plugin, sticker_id, self.has_private_access())
	}

	fn save_attachment(&mut self, m: MessageRef, attachment_id: u64) -> Result<(), HostError> {
		self.require(Permission::ActionsSave)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.save_attachment(&self.plugin, &m, attachment_id, self.has_private_access())
	}

	fn join_voice(&mut self, guild_id: Option<u64>, channel_id: u64) -> Result<(), HostError> {
		self.require(Permission::ActionsVoice)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.join_voice(&self.plugin, guild_id, channel_id)
	}

	fn leave_voice(&mut self) -> Result<(), HostError> {
		self.require(Permission::ActionsVoice)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services.broker.leave_voice(&self.plugin)
	}

	fn set_guild_notifications(
		&mut self,
		guild_id: u64,
		settings: NotificationSettings,
	) -> Result<(), HostError> {
		self.require(Permission::ActionsSettings)?;
		check_notification_settings(&settings)?;
		within_rate(self.rate.settings.take())?;
		self.services
			.broker
			.guild_notifications(&self.plugin, guild_id, settings)
	}

	fn set_channel_notifications(
		&mut self,
		channel_id: u64,
		settings: NotificationSettings,
	) -> Result<(), HostError> {
		self.require(Permission::ActionsSettings)?;
		check_notification_settings(&settings)?;
		within_rate(self.rate.settings.take())?;
		self.services
			.broker
			.channel_notifications(&self.plugin, channel_id, settings)
	}

	fn set_client_option(&mut self, value: ClientOption) -> Result<(), HostError> {
		self.require(Permission::ActionsSettings)?;
		let in_range = match &value {
			ClientOption::NotificationVolume(volume) => *volume <= MAX_NOTIFICATION_VOLUME,
			ClientOption::ForumLayout(layout) => *layout <= MAX_FORUM_LAYOUT,
			ClientOption::ForumSort(sort) => *sort <= MAX_FORUM_SORT,
			_ => true,
		};
		if !in_range {
			return Err(invalid("option value out of range"));
		}
		within_rate(self.rate.settings.take())?;
		self.services.broker.set_client_option(&self.plugin, value)
	}

	fn set_dm_sections(&mut self, sections: Vec<DmSection>) -> Result<(), HostError> {
		self.require(Permission::ActionsSettings)?;
		let mut budget = MAX_DM_CHANNELS;
		let sections: Vec<DmSection> = sections
			.into_iter()
			.take(MAX_DM_SECTIONS)
			.filter_map(|section| {
				let title = clip_line(&section.title, MAX_DM_SECTION_TITLE);
				let take = section.channels.len().min(budget);
				budget -= take;
				(!title.trim().is_empty()).then(|| DmSection {
					title,
					channels: section.channels.into_iter().take(take).collect(),
					collapsed: section.collapsed,
				})
			})
			.collect();
		within_rate(self.rate.settings.take())?;
		self.services.broker.set_dm_sections(&self.plugin, sections)
	}

	fn clone_emoji(&mut self, guild_id: u64, emoji: EmojiRef, name: String) -> Result<(), HostError> {
		self.require(Permission::ActionsGuild)?;
		self.require_gesture()?;
		if emoji.id.is_none() {
			return Err(invalid("only custom emoji can be cloned"));
		}
		if !valid_expression_name(&name) {
			return Err(invalid(
				"emoji name must be 2 to 32 letters, digits or underscores",
			));
		}
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.clone_emoji(&self.plugin, guild_id, &emoji, name)
	}

	fn clone_sticker(&mut self, guild_id: u64, sticker_id: u64) -> Result<(), HostError> {
		self.require(Permission::ActionsGuild)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services
			.broker
			.clone_sticker(&self.plugin, guild_id, sticker_id)
	}

	fn register_keybind(&mut self, bind: Keybind) -> Result<(), HostError> {
		self.require(Permission::UiKeybinds)?;
		within_rate(self.rate.ui.take())?;
		self.services
			.keybinds
			.register(&self.plugin, &bind)
			.map_err(|error| invalid(error.reason()))
	}

	fn unregister_keybind(&mut self, id: String) -> Result<(), HostError> {
		self.require(Permission::UiKeybinds)?;
		self.services.keybinds.unregister(&self.plugin, &id);
		Ok(())
	}

	fn speak(&mut self, text: String, rate: f32, volume: f32) -> Result<(), HostError> {
		self.require(Permission::ActionsSpeak)?;
		if text.trim().is_empty() || text.chars().count() > MAX_SPOKEN_CHARS {
			return Err(invalid("speech text length"));
		}
		if !rate.is_finite() || !volume.is_finite() {
			return Err(invalid("speech rate and volume must be finite"));
		}
		within_rate(self.rate.speak.take())?;
		self.services
			.broker
			.speak(&self.plugin, &text, rate.clamp(0.5, 2.0), volume.clamp(0.0, 1.0))
	}

	fn invalidate_message(&mut self, m: MessageRef) -> Result<(), HostError> {
		self.invalidate(Invalidation::Message(m.channel_id, m.id))
	}

	fn invalidate_user(&mut self, user_id: u64) -> Result<(), HostError> {
		self.invalidate(Invalidation::User(user_id))
	}

	fn invalidate_channel(&mut self, channel_id: u64) -> Result<(), HostError> {
		self.invalidate(Invalidation::Channel(channel_id))
	}

	fn open_settings(&mut self, page: String) -> Result<(), HostError> {
		self.require(Permission::ActionsNavigate)?;
		self.require_gesture()?;
		if !SETTINGS_PAGES.contains(&page.as_str()) {
			return Err(invalid("unknown settings page"));
		}
		within_rate(self.rate.action.take())?;
		self.services.broker.open_settings(&self.plugin, &page)
	}

	fn navigate_back(&mut self) -> Result<(), HostError> {
		self.navigate(Navigation::Back)
	}

	fn navigate_forward(&mut self) -> Result<(), HostError> {
		self.navigate(Navigation::Forward)
	}
}

impl HostState {
	fn remember_pick(&mut self, path: PathBuf) -> u64 {
		let handle = self.next_pick;
		self.next_pick += 1;
		self.picked.insert(handle, path);
		while self.picked.len() > MAX_PICKED_HANDLES {
			self.picked.pop_first();
		}
		handle
	}

	fn invalidate(&mut self, target: Invalidation) -> Result<(), HostError> {
		within_rate(self.rate.invalidate.take())?;
		self.services.broker.invalidate(&self.plugin, target)
	}

	fn navigate(&mut self, direction: Navigation) -> Result<(), HostError> {
		self.require(Permission::ActionsNavigate)?;
		self.require_gesture()?;
		within_rate(self.rate.action.take())?;
		self.services.broker.navigate(&self.plugin, direction)
	}
}

fn epoch_millis() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_millis() as i64)
}

fn valid_emoji(emoji: &EmojiRef) -> bool {
	!emoji.name.is_empty() && emoji.name.len() <= MAX_EMOJI_BYTES
}

fn valid_expression_name(name: &str) -> bool {
	(MIN_EMOJI_NAME..=MAX_EMOJI_NAME).contains(&name.len())
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_file_name(name: &str) -> bool {
	!name.is_empty()
		&& name.len() <= MAX_FILE_NAME_BYTES
		&& !name.chars().any(|character| {
			character.is_control()
				|| matches!(character, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
		})
}

fn check_blobs(files: &[FileBlob]) -> Result<(), HostError> {
	if files.is_empty() || files.len() > MAX_UPLOAD_FILES {
		return Err(invalid("file count"));
	}
	if files.iter().any(|file| !valid_file_name(&file.name)) {
		return Err(invalid("file name"));
	}
	let total: usize = files.iter().map(|file| file.bytes.len()).sum();
	if total > MAX_UPLOAD_BYTES {
		return Err(invalid("files are too large"));
	}
	Ok(())
}

fn check_notification_settings(settings: &NotificationSettings) -> Result<(), HostError> {
	if settings.level.is_some_and(|level| level > MAX_NOTIFICATION_LEVEL) {
		return Err(invalid("notification level"));
	}
	Ok(())
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
