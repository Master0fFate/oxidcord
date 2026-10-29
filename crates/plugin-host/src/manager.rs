use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use markdown::Node;
use parking_lot::{Mutex, RwLock};
use semver::Version;
use url::Url;

use crate::api::model::{
	Badge, ChannelClick, ChannelDetail, ClientEvent, ContextDetail, DisplayOverrideFull, EmojiInfo, EmojiRef,
	Lifecycle, LinkVerdict, MessageClick, MessageFull, MessageMeta, NodeKind, NotificationInfo,
	NotificationOverride, NotificationVerdict, OutgoingFull, PickVerdict, PluginSummary, Presence, RichNode,
	RichTree, SendDecisionFull, TextNode, TextSize, Tone, UiContext, ViewEvent, ViewNode, ViewReply,
	ViewTree, Weight,
};
use crate::api::{
	DisplayOverride, Event, MdNode, MessageView, OutgoingMessage, SendDecision as GuestDecision, User,
	VoiceInfo,
};
use crate::broker::HostBroker;
use crate::convert::{
	ExtContext, ImagePolicy, SanitizedPatch, clip, clip_line, md_to_rich, md_to_tree, sanitize_badges,
	sanitize_display_full, sanitize_patch, sanitize_patch_ext, sanitize_view, tree_to_md, upgrade_ui_tree,
	validate_ui_tree, web_url,
};
use crate::error::{Error, Result};
use crate::hook::Hook;
use crate::instance::{
	CallError, CallKind, DisplayRequest, Health, InterceptItem, Output, PluginInstance, PluginState,
};
use crate::manifest::{Manifest, PermissionRequest};
use crate::mounts::{self, Overlay, ViewContext};
use crate::package::{Package, Trust, discover};
use crate::patches::PluginPatch;
use crate::permission::Permission;
use crate::runtime::Runtime;
use crate::slots::{CommandEntry, Slots};

const MAX_OUTGOING_BYTES: usize = 4_000;
const MAX_DISPLAY_NAME_CHARS: usize = 80;
const MAX_BADGE_CHARS: usize = 16;
const INTERCEPT_BUDGET: Duration = Duration::from_millis(150);
const INTERCEPT_BATCH_BUDGET: Duration = Duration::from_millis(750);
const BREAKER_LIMIT: u32 = 3;
const MAX_STICKERS: usize = 3;
const MAX_ATTACHMENT_NAME: usize = 255;
const MAX_NOTIFICATION_TITLE: usize = 100;
const MAX_NOTIFICATION_BODY: usize = 500;
const MAX_LINK_BYTES: usize = 2_048;
const MAX_SELECTION_BYTES: usize = 4_000;
const MAX_INSERT_BYTES: usize = 2_000;
const MAX_TOAST_TEXT: usize = 200;
const DISPLAY_CHUNK: usize = 50;
const MAX_AUTOCOMPLETE_CANDIDATES: usize = 200;

pub struct Intercepted {
	pub ast: Vec<Node>,
	pub retain: bool,
	pub patches: Vec<PluginPatch>,
}

pub struct InterceptInput {
	pub full: MessageFull,
	pub ast: Vec<Node>,
	pub meta: Option<MessageMeta>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayAnswer {
	Legacy(DisplayOverride),
	Full(Arc<str>, DisplayOverrideFull),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelBadges {
	pub plugin_id: Arc<str>,
	pub badges: Vec<Vec<Badge>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HookFlags {
	pub notification: bool,
	pub draft: bool,
	pub decorate: bool,
	pub autocomplete: bool,
	pub link_click: bool,
	pub client_events: bool,
	pub presence_events: bool,
	pub read_state_events: bool,
	pub intercept_ext: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmojiPick {
	Default,
	Insert { by: String, text: String },
	Blocked { by: String, reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FullSendDecision {
	Pass,
	Replaced { by: String, msg: OutgoingFull },
	Cancelled { by: String, reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
	pub id: String,
	pub name: String,
	pub version: Version,
	pub state: PluginState,
	pub permissions: Vec<PermissionRequest>,
	pub description: String,
	pub trust: Trust,
	pub enabled: bool,
	pub granted: Vec<Permission>,
	pub network_hosts: Vec<String>,
	pub image_hosts: Vec<String>,
	pub granted_hosts: Vec<String>,
	pub hooks: Vec<Hook>,
	pub url_schemes: Vec<String>,
	pub needs_prompt: bool,
	pub error: Option<String>,
	pub crashes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginDebug {
	pub id: String,
	pub calls: u64,
	pub last_ms: f64,
	pub denied: u64,
	pub crashes: u64,
}

#[derive(Debug)]
pub struct LoadIssue {
	pub dir: PathBuf,
	pub error: Error,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
	#[error("unknown command")]
	Unknown,
	#[error("{0}")]
	Plugin(String),
	#[error("{0}")]
	Call(#[from] CallError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Breaker {
	Intercept,
	Send,
	Display,
	Notification,
	LinkClick,
	Autocomplete,
	Decorate,
}

impl Breaker {
	const COUNT: usize = 7;

	fn label(self) -> &'static str {
		match self {
			Self::Intercept => "intercept",
			Self::Send => "before-send",
			Self::Display => "display-user",
			Self::Notification => "notification",
			Self::LinkClick => "link-click",
			Self::Autocomplete => "autocomplete-emoji",
			Self::Decorate => "decorate-channel",
		}
	}

	fn of(hook: Hook) -> Option<Self> {
		match hook {
			Hook::Intercept => Some(Self::Intercept),
			Hook::BeforeSend => Some(Self::Send),
			Hook::DisplayUser => Some(Self::Display),
			Hook::Notification => Some(Self::Notification),
			Hook::LinkClick => Some(Self::LinkClick),
			Hook::AutocompleteEmoji => Some(Self::Autocomplete),
			Hook::DecorateChannel => Some(Self::Decorate),
			Hook::DraftChange | Hook::MessageClick | Hook::ChannelClick => None,
		}
	}
}

struct LoadedPlugin {
	manifest: Arc<Manifest>,
	dir: PathBuf,
	trust: Trust,
	health: Arc<Health>,
	broker: Arc<dyn HostBroker>,
	instance: Mutex<Option<Arc<PluginInstance>>>,
	load_error: Option<String>,
	component: Option<wasmtime::component::Component>,
	breakers: [AtomicU32; Breaker::COUNT],
}

impl LoadedPlugin {
	fn instance(&self) -> Option<Arc<PluginInstance>> {
		self.instance.lock().clone()
	}

	fn is_active(&self) -> bool {
		self.health.state().is_active()
	}

	fn tripped(&self, breaker: Breaker) -> bool {
		self.breakers[breaker as usize].load(Ordering::Relaxed) >= BREAKER_LIMIT
	}

	fn succeeded(&self, breaker: Breaker) {
		self.breakers[breaker as usize].store(0, Ordering::Relaxed);
	}

	fn failed(&self, breaker: Breaker, error: &CallError) {
		tracing::debug!(target: "plugin", plugin = %self.manifest.id, hook = breaker.label(), "skipped: {error}");
		if matches!(error, CallError::Timeout)
			&& self.breakers[breaker as usize].fetch_add(1, Ordering::Relaxed) + 1 == BREAKER_LIMIT
		{
			tracing::warn!(target: "plugin", plugin = %self.manifest.id, hook = breaker.label(), "hook disabled after repeated timeouts");
			self.health.invalidate_hooks();
			self.broker.hooks_changed();
		}
	}
}

#[derive(Clone)]
struct HookEntry {
	id: Arc<str>,
	plugin: Arc<LoadedPlugin>,
	instance: Arc<PluginInstance>,
	images: Arc<ImagePolicy>,
	private: bool,
	timeline: bool,
	retain: bool,
	basic: bool,
	voice: bool,
	relationships: bool,
	presence: bool,
	directory: bool,
	read_state: bool,
	ext_send: bool,
	ext_display: bool,
	ext_intercept: bool,
}

impl HookEntry {
	fn sees(&self, guild_id: Option<u64>) -> bool {
		guild_id.is_some() || self.private
	}

	fn sees_event(&self, event: &Event) -> bool {
		let timeline = |guild_id: Option<u64>| self.timeline && self.sees(guild_id);
		match event {
			Event::MessageCreated(message) | Event::MessageUpdated(message) => timeline(message.guild_id),
			Event::MessageDeleted(message) => timeline(message.guild_id),
			Event::ReactionAdded(reaction) => timeline(reaction.message.guild_id),
			Event::Typing(_) => self.timeline,
			Event::ChannelSwitched(switch) => self.basic && self.sees(switch.guild_id),
			Event::VoiceChanged(info) => self.voice && self.sees(info.guild_id),
			Event::SettingChanged(_) | Event::Connection(_) => true,
		}
	}

	fn sees_client_event(&self, event: &ClientEvent, private_scope: bool) -> bool {
		match event {
			ClientEvent::RelationshipChanged(_) | ClientEvent::RelationshipRemoved(_) => self.relationships,
			ClientEvent::GuildJoined(_)
			| ClientEvent::GuildLeft(_)
			| ClientEvent::GuildRemoved(_)
			| ClientEvent::ChannelListChanged => self.directory,
			ClientEvent::DmClosed(_) => self.directory && self.private,
			ClientEvent::PresenceChanged(_) | ClientEvent::IdleChanged(_) => self.presence,
			ClientEvent::ReactionRemoved(reaction) => self.timeline && self.sees(reaction.message.guild_id),
			ClientEvent::MessageAcked((message, _)) => self.timeline && self.sees(message.guild_id),
			ClientEvent::UnreadChanged(_) => self.read_state && (self.private || !private_scope),
			ClientEvent::Ready => true,
		}
	}
}

#[derive(Default)]
struct HookSets {
	stamp: (u64, u64),
	events: Vec<HookEntry>,
	render: Vec<HookEntry>,
	filter: Vec<HookEntry>,
	intercept: Vec<HookEntry>,
	send: Vec<HookEntry>,
	display: Vec<HookEntry>,
	emoji: Vec<HookEntry>,
	events2: Vec<HookEntry>,
	draft: Vec<HookEntry>,
	autocomplete: Vec<HookEntry>,
	decorate: Vec<HookEntry>,
	notification: Vec<HookEntry>,
	message_click: Vec<HookEntry>,
	channel_click: Vec<HookEntry>,
	link_click: Vec<HookEntry>,
	hidden_channels: bool,
	hidden_typing: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderHooks {
	pub filter: bool,
	pub render: bool,
	pub intercept: bool,
}

impl RenderHooks {
	pub fn any(self) -> bool {
		self.filter || self.render || self.intercept
	}
}

pub struct PluginManager {
	runtime: Runtime,
	plugins: RwLock<Vec<Arc<LoadedPlugin>>>,
	safe_mode: Mutex<bool>,
	hook_revision: Arc<AtomicU64>,
	hooks: RwLock<Arc<HookSets>>,
}

impl PluginManager {
	pub fn new(runtime: Runtime) -> Self {
		Self {
			runtime,
			plugins: RwLock::new(Vec::new()),
			safe_mode: Mutex::new(false),
			hook_revision: Arc::new(AtomicU64::new(0)),
			hooks: RwLock::new(Arc::new(HookSets {
				stamp: (u64::MAX, u64::MAX),
				..HookSets::default()
			})),
		}
	}

	fn rehook(&self) {
		self.hook_revision.fetch_add(1, Ordering::AcqRel);
	}

	fn hooks(&self) -> Arc<HookSets> {
		let stamp = (
			self.hook_revision.load(Ordering::Acquire),
			self.runtime.grants().revision(),
		);
		{
			let current = self.hooks.read();
			if current.stamp == stamp {
				return Arc::clone(&current);
			}
		}
		let mut slot = self.hooks.write();
		if slot.stamp != stamp {
			*slot = Arc::new(self.build_hooks(stamp));
		}
		Arc::clone(&slot)
	}

	fn build_hooks(&self, stamp: (u64, u64)) -> HookSets {
		let grants = self.runtime.grants();
		let mut sets = HookSets {
			stamp,
			..HookSets::default()
		};
		for plugin in self.plugins.read().iter() {
			if !plugin.is_active() {
				continue;
			}
			let Some(instance) = plugin.instance() else {
				continue;
			};
			let name = plugin.manifest.id.as_str();
			let has = |permission| grants.has(name, permission);
			let ext = instance.is_ext();
			let hooks2 = instance.has_hooks2();
			let declares = |hook: Hook| {
				hooks2
					&& plugin.manifest.hooks.contains(&hook)
					&& has(hook.permission())
					&& Breaker::of(hook).is_none_or(|breaker| !plugin.tripped(breaker))
			};
			let entry = HookEntry {
				id: Arc::from(name),
				plugin: Arc::clone(plugin),
				instance,
				images: grants.image_policy(name),
				private: has(Permission::ContextPrivate),
				timeline: has(Permission::ContextTimeline),
				retain: has(Permission::MessageRetain),
				basic: has(Permission::ContextBasic),
				voice: has(Permission::ContextVoice),
				relationships: has(Permission::ContextRelationships),
				presence: has(Permission::ContextPresence),
				directory: has(Permission::ContextDirectory),
				read_state: has(Permission::ContextReadState),
				ext_send: declares(Hook::BeforeSend),
				ext_display: declares(Hook::DisplayUser),
				ext_intercept: declares(Hook::Intercept),
			};
			sets.hidden_channels |= has(Permission::ChannelsHidden);
			sets.hidden_typing |= has(Permission::TypingHidden);
			let targets = [
				(Permission::MessageRender, true, &mut sets.render),
				(Permission::MessageFilter, true, &mut sets.filter),
				(
					Permission::MessageIntercept,
					ext && !plugin.tripped(Breaker::Intercept),
					&mut sets.intercept,
				),
				(
					Permission::MessageSend,
					!plugin.tripped(Breaker::Send),
					&mut sets.send,
				),
				(
					Permission::UserDisplay,
					!plugin.tripped(Breaker::Display),
					&mut sets.display,
				),
				(Permission::Composer, ext, &mut sets.emoji),
			];
			for (permission, capable, set) in targets {
				if capable && has(permission) {
					set.push(entry.clone());
				}
			}
			if hooks2 {
				let targets = [
					(Hook::DraftChange, &mut sets.draft),
					(Hook::AutocompleteEmoji, &mut sets.autocomplete),
					(Hook::DecorateChannel, &mut sets.decorate),
					(Hook::Notification, &mut sets.notification),
					(Hook::MessageClick, &mut sets.message_click),
					(Hook::ChannelClick, &mut sets.channel_click),
					(Hook::LinkClick, &mut sets.link_click),
				];
				for (hook, set) in targets {
					if declares(hook) {
						set.push(entry.clone());
					}
				}
				sets.events2.push(entry.clone());
			}
			sets.events.push(entry);
		}
		sets
	}

	pub fn hook_flags(&self) -> HookFlags {
		let sets = self.hooks();
		HookFlags {
			notification: !sets.notification.is_empty(),
			draft: !sets.draft.is_empty(),
			decorate: !sets.decorate.is_empty(),
			autocomplete: !sets.autocomplete.is_empty(),
			link_click: !sets.link_click.is_empty(),
			client_events: !sets.events2.is_empty(),
			presence_events: sets.events2.iter().any(|entry| entry.presence),
			read_state_events: sets.events2.iter().any(|entry| entry.read_state),
			intercept_ext: sets.intercept.iter().any(|entry| entry.ext_intercept),
		}
	}

	pub fn runtime(&self) -> &Runtime {
		&self.runtime
	}

	pub fn slots(&self) -> &Arc<Slots> {
		self.runtime.slots()
	}

	pub fn commands(&self) -> Arc<[(String, CommandEntry)]> {
		self.runtime.commands().list()
	}

	pub fn interactive_call_p95_ms(&self) -> Option<f64> {
		self.runtime.latency().p95_ms()
	}

	pub fn load_all(&self, plugins_dir: &Path, safe_mode: bool) -> Vec<LoadIssue> {
		*self.safe_mode.lock() = safe_mode;
		let mut loaded = Vec::new();
		let mut issues = Vec::new();
		for dir in discover(plugins_dir) {
			match self.load_dir(&dir, safe_mode) {
				Ok(plugin) => loaded.push(plugin),
				Err(error) => issues.push(LoadIssue { dir, error }),
			}
		}
		loaded.sort_by(|left, right| {
			(left.manifest.priority, &left.manifest.id).cmp(&(right.manifest.priority, &right.manifest.id))
		});
		let previous = std::mem::replace(&mut *self.plugins.write(), loaded);
		for plugin in previous {
			self.retire(&plugin);
		}
		self.rehook();
		for plugin in self.plugins.read().iter() {
			if let Err(error) = self.start(plugin) {
				tracing::warn!(target: "plugin", plugin = %plugin.manifest.id, "{error}");
			}
		}
		issues
	}

	pub fn load_one(&self, dir: &Path) -> Result<String> {
		let plugin = self.load_dir(dir, *self.safe_mode.lock())?;
		let id = plugin.manifest.id.clone();
		let existing = self
			.plugins
			.read()
			.iter()
			.find(|loaded| loaded.manifest.id == id)
			.cloned();
		match existing {
			Some(old) => self.replace(&old, Arc::clone(&plugin)),
			None => {
				let mut plugins = self.plugins.write();
				plugins.push(Arc::clone(&plugin));
				plugins.sort_by(|left, right| {
					(left.manifest.priority, &left.manifest.id)
						.cmp(&(right.manifest.priority, &right.manifest.id))
				});
			}
		}
		self.rehook();
		self.start(&plugin)?;
		Ok(id)
	}

	pub fn unload(&self, id: &str) -> Result<PathBuf> {
		let plugin = self.find(id)?;
		self.retire(&plugin);
		self.plugins
			.write()
			.retain(|loaded| !Arc::ptr_eq(loaded, &plugin));
		self.rehook();
		Ok(plugin.dir.clone())
	}

	pub fn dirs(&self) -> Vec<(String, PathBuf, PluginState)> {
		self.plugins
			.read()
			.iter()
			.map(|plugin| {
				(
					plugin.manifest.id.clone(),
					plugin.dir.clone(),
					plugin.health.state(),
				)
			})
			.collect()
	}

	pub fn has_active(&self, permission: Permission) -> bool {
		let sets = self.hooks();
		match permission {
			Permission::MessageRender => !sets.render.is_empty(),
			Permission::MessageFilter => !sets.filter.is_empty(),
			Permission::MessageSend => !sets.send.is_empty(),
			Permission::UserDisplay => !sets.display.is_empty(),
			other => {
				let grants = self.runtime.grants();
				sets.events.iter().any(|entry| grants.has(&entry.id, other))
			}
		}
	}

	pub fn has_instances(&self) -> bool {
		!self.hooks().events.is_empty()
	}

	pub fn has_timeline_events(&self) -> bool {
		self.hooks().events.iter().any(|entry| entry.timeline)
	}

	pub fn set_setting(&self, id: &str, key: &str, value: &str) -> Result<()> {
		self.instance(id)?
			.set_setting(key.to_owned(), value.to_owned())
			.map_err(|error| Error::wasm(format!("{id}: {error}")))
	}

	fn load_dir(&self, dir: &Path, safe_mode: bool) -> Result<Arc<LoadedPlugin>> {
		let package = Package::from_dir(dir)?;
		let manifest = Arc::new(package.manifest.clone());
		let config = self.runtime.config();
		let trust = package.verify(config.catalog_key.as_ref());
		let compatible = manifest.supports_api()
			&& manifest
				.min_host
				.as_ref()
				.is_none_or(|minimum| *minimum <= config.host_version);
		let enabled = self.runtime.grants().get(&manifest.id).enabled && !safe_mode;
		let (state, component, load_error) = if !compatible {
			(
				PluginState::Incompatible,
				None,
				Some("requires a newer host or a different plugin api".to_owned()),
			)
		} else if !enabled {
			(PluginState::Disabled, None, None)
		} else {
			match package.wasm.as_deref().map(|wasm| self.runtime.compile(wasm)) {
				Some(Ok(component)) => (PluginState::Loaded, Some(component), None),
				Some(Err(error)) => (PluginState::Incompatible, None, Some(error.to_string())),
				None => (PluginState::Loaded, None, None),
			}
		};
		Ok(Arc::new(LoadedPlugin {
			manifest,
			dir: dir.to_owned(),
			trust,
			health: Arc::new(Health::new(state, Arc::clone(&self.hook_revision))),
			broker: Arc::clone(&self.runtime.shared().services.broker),
			instance: Mutex::new(None),
			load_error,
			component,
			breakers: std::array::from_fn(|_| AtomicU32::new(0)),
		}))
	}

	fn start(&self, plugin: &Arc<LoadedPlugin>) -> Result<()> {
		let Some(component) = &plugin.component else {
			return Ok(());
		};
		if plugin.health.state() == PluginState::Incompatible {
			return Ok(());
		}
		let id: Arc<str> = Arc::from(plugin.manifest.id.as_str());
		let instance = PluginInstance::new(
			Arc::clone(&id),
			Arc::clone(self.runtime.shared()),
			component,
			Arc::clone(&plugin.health),
		)
		.inspect_err(|error| {
			if let Error::Incompatible(detail) = error {
				plugin.health.mark_incompatible(detail);
			}
		})?;
		self.runtime.timers().attach(&id, Arc::downgrade(&instance));
		*plugin.instance.lock() = Some(Arc::clone(&instance));
		plugin.health.set_state(PluginState::Loaded);
		self.rehook();
		instance.start().map_err(|error| match error {
			CallError::Incompatible(detail) => Error::Incompatible(detail),
			other => Error::wasm(format!("{}: {other}", plugin.manifest.id)),
		})
	}

	fn retire(&self, plugin: &Arc<LoadedPlugin>) {
		if let Some(instance) = plugin.instance.lock().take() {
			instance.shutdown();
		}
		let id = &plugin.manifest.id;
		self.runtime.slots().clear_plugin(id);
		self.runtime.commands().clear_plugin(id);
		self.runtime.mounts().clear_plugin(id);
		self.runtime.patches().clear_plugin(id);
		self.runtime.keybinds().clear_plugin(id);
		self.runtime.watches().clear_plugin(id);
		self.runtime.timers().detach(id);
		self.rehook();
	}

	fn find(&self, id: &str) -> Result<Arc<LoadedPlugin>> {
		self.plugins
			.read()
			.iter()
			.find(|plugin| plugin.manifest.id == id)
			.cloned()
			.ok_or_else(|| Error::UnknownPlugin(id.to_owned()))
	}

	pub fn enable(&self, id: &str) -> Result<()> {
		if *self.safe_mode.lock() {
			return Err(Error::NotRunning(format!("{id} (safe mode)")));
		}
		let current = self.find(id)?;
		self.runtime.grants().set_enabled(id, true)?;
		let fresh = self.load_dir(&current.dir, false)?;
		self.replace(&current, Arc::clone(&fresh));
		self.start(&fresh)
	}

	pub fn disable(&self, id: &str) -> Result<()> {
		let plugin = self.find(id)?;
		self.runtime.grants().set_enabled(id, false)?;
		self.retire(&plugin);
		if plugin.health.state() != PluginState::Incompatible {
			plugin.health.set_state(PluginState::Disabled);
		}
		Ok(())
	}

	pub fn reload(&self, id: &str) -> Result<()> {
		let current = self.find(id)?;
		let fresh = self.load_dir(&current.dir, *self.safe_mode.lock())?;
		self.replace(&current, Arc::clone(&fresh));
		self.start(&fresh)
	}

	fn replace(&self, old: &Arc<LoadedPlugin>, fresh: Arc<LoadedPlugin>) {
		self.retire(old);
		let mut plugins = self.plugins.write();
		if let Some(slot) = plugins.iter_mut().find(|plugin| Arc::ptr_eq(plugin, old)) {
			*slot = fresh;
		}
		plugins.sort_by(|left, right| {
			(left.manifest.priority, &left.manifest.id).cmp(&(right.manifest.priority, &right.manifest.id))
		});
		drop(plugins);
		self.rehook();
	}

	pub fn list(&self) -> Vec<PluginInfo> {
		let grants = self.runtime.grants();
		self.plugins
			.read()
			.iter()
			.map(|plugin| {
				let grant = grants.get(&plugin.manifest.id);
				PluginInfo {
					id: plugin.manifest.id.clone(),
					name: plugin.manifest.name.clone(),
					version: plugin.manifest.version.clone(),
					state: plugin.health.state(),
					permissions: plugin.manifest.permissions.clone(),
					description: plugin.manifest.description.clone(),
					trust: plugin.trust,
					enabled: grant.enabled,
					needs_prompt: grants.requires_reprompt(&plugin.manifest),
					granted: grant.permissions.iter().copied().collect(),
					network_hosts: plugin.manifest.network_hosts.clone(),
					image_hosts: plugin.manifest.image_hosts.clone(),
					granted_hosts: grant
						.network_hosts
						.iter()
						.chain(&grant.image_hosts)
						.cloned()
						.collect(),
					hooks: plugin.manifest.hooks.clone(),
					url_schemes: plugin.manifest.url_schemes.clone(),
					error: plugin.load_error.clone().or_else(|| plugin.health.last_error()),
					crashes: plugin.health.total_crashes(),
				}
			})
			.collect()
	}

	pub fn summaries(&self) -> Vec<PluginSummary> {
		let grants = self.runtime.grants();
		self.plugins
			.read()
			.iter()
			.map(|plugin| PluginSummary {
				id: plugin.manifest.id.clone(),
				name: plugin.manifest.name.clone(),
				version: plugin.manifest.version.to_string(),
				enabled: grants.get(&plugin.manifest.id).enabled,
				ext: !plugin.manifest.hooks.is_empty(),
			})
			.collect()
	}

	pub fn debug_snapshot(&self) -> Vec<PluginDebug> {
		self.plugins
			.read()
			.iter()
			.map(|plugin| PluginDebug {
				id: plugin.manifest.id.clone(),
				calls: plugin.health.calls(),
				last_ms: plugin.health.last_ms(),
				denied: plugin.health.denied(),
				crashes: plugin.health.total_crashes(),
			})
			.collect()
	}

	pub fn manifest(&self, id: &str) -> Result<Arc<Manifest>> {
		Ok(Arc::clone(&self.find(id)?.manifest))
	}

	pub fn instance(&self, id: &str) -> Result<Arc<PluginInstance>> {
		self.find(id)?
			.instance()
			.ok_or_else(|| Error::NotRunning(id.to_owned()))
	}

	pub fn dispatch_event(&self, event: Event) {
		self.dispatch_event_scoped(event, false);
	}

	pub fn dispatch_event_scoped(&self, event: Event, private_scope: bool) {
		let sets = self.hooks();
		let event = Arc::new(event);
		let visible = |entry: &&HookEntry| entry.sees_event(&event) && (entry.private || !private_scope);
		for entry in sets.events.iter().filter(visible) {
			entry.instance.post_event(Arc::clone(&event));
		}
	}

	pub fn dispatch_voice_changed(&self, info: VoiceInfo) {
		self.dispatch_event(Event::VoiceChanged(info));
	}

	pub fn render_hooks(&self, guild_id: Option<u64>) -> RenderHooks {
		let sets = self.hooks();
		let visible = |entries: &[HookEntry]| entries.iter().any(|entry| entry.sees(guild_id));
		RenderHooks {
			filter: visible(&sets.filter),
			render: visible(&sets.render),
			intercept: visible(&sets.intercept),
		}
	}

	pub fn render_message(&self, view: &Arc<MessageView>, ast: Vec<Node>) -> Vec<Node> {
		let sets = self.hooks();
		let plugins = sets.render.iter().filter(|entry| entry.sees(view.guild_id));
		render_chain(plugins, view, &ast).unwrap_or(ast)
	}

	pub fn render_deleted(&self, view: &Arc<MessageView>, ast: &[Node]) -> Option<Vec<Node>> {
		let sets = self.hooks();
		let plugins = sets
			.render
			.iter()
			.filter(|entry| entry.sees(view.guild_id) && entry.timeline);
		render_chain(plugins, view, ast)
	}

	pub fn has_message_log(&self) -> bool {
		let sets = self.hooks();
		sets.render.iter().any(|entry| entry.timeline) || sets.intercept.iter().any(|entry| entry.retain)
	}

	pub fn has_intercept(&self) -> bool {
		!self.hooks().intercept.is_empty()
	}

	pub fn reveals_hidden_channels(&self) -> bool {
		self.hooks().hidden_channels
	}

	pub fn hides_typing(&self) -> bool {
		self.hooks().hidden_typing
	}

	pub fn has_emoji_pick(&self) -> bool {
		!self.hooks().emoji.is_empty()
	}

	fn image_policy(&self, plugin_id: &str) -> Arc<ImagePolicy> {
		self.runtime.grants().image_policy(plugin_id)
	}

	pub fn intercept_many(&self, inputs: Vec<InterceptInput>) -> Vec<Intercepted> {
		let sets = self.hooks();
		if sets.intercept.is_empty() {
			return inputs
				.into_iter()
				.map(|input| Intercepted {
					ast: input.ast,
					retain: false,
					patches: Vec::new(),
				})
				.collect();
		}
		let mut work: Vec<InterceptWork> = inputs.into_iter().map(InterceptWork::new).collect();
		let started = Instant::now();
		let budget = INTERCEPT_BUDGET
			.saturating_mul(u32::try_from(work.len()).unwrap_or(u32::MAX))
			.min(INTERCEPT_BATCH_BUDGET);
		for entry in &sets.intercept {
			if started.elapsed() >= budget {
				tracing::debug!(target: "plugin", "intercept budget spent, remaining plugins skipped");
				break;
			}
			if entry.plugin.tripped(Breaker::Intercept) {
				continue;
			}
			let visible: Vec<usize> = (0..work.len())
				.filter(|&index| entry.sees(work[index].full.view.guild_id))
				.collect();
			if visible.is_empty() {
				continue;
			}
			if entry.ext_intercept {
				intercept_ext(entry, &mut work, &visible);
			} else {
				intercept_plain(entry, &mut work, &visible);
			}
		}
		work.into_iter().map(|item| item.outcome).collect()
	}

	pub fn store_patches(&self, message_id: u64, patches: Vec<PluginPatch>) {
		self.runtime.patches().set(message_id, patches);
	}

	fn hides_private(&self, plugin_id: &str, guild_id: Option<u64>) -> bool {
		guild_id.is_none() && !self.runtime.grants().has(plugin_id, Permission::ContextPrivate)
	}

	pub fn view_event(&self, plugin_id: &str, event: ViewEvent, detail: Option<ContextDetail>) -> Result<()> {
		let instance = self.instance(plugin_id)?;
		let point = event.point.clone();
		let mount = event.mount.clone();
		let hide_private = self.hides_private(plugin_id, event.context.guild_id);
		let detail = detail.map(|detail| sanitize_detail(detail, hide_private));
		let context = ViewContext {
			guild_id: event.context.guild_id,
			channel_id: event.context.channel_id,
			message_id: event.context.message_id,
			user_id: event.context.user_id,
			attachment_id: detail.as_ref().and_then(|detail| detail.attachment_id),
		};
		let reply = instance
			.view_event(event, detail)
			.map_err(|error| Error::wasm(format!("{plugin_id}: {error}")))?;
		self.apply_reply(plugin_id, &point, &mount, context, reply);
		Ok(())
	}

	fn apply_reply(&self, plugin_id: &str, point: &str, mount: &str, context: ViewContext, reply: ViewReply) {
		let owner: Arc<str> = Arc::from(plugin_id);
		let surfaces = self.runtime.mounts();
		if let Some(tree) = reply.replace {
			let tree = sanitize_view(&tree, &self.image_policy(plugin_id));
			if let Some(point) = mounts::static_point(point) {
				surfaces.update(point, plugin_id, mount, tree).ok();
			} else if let Some(point) = mounts::context_point(point) {
				if !self.hides_private(plugin_id, context.guild_id) {
					surfaces.store_view(&owner, point, context, Some(tree));
				}
			} else if point == "modal" {
				surfaces.push_overlay(Overlay::Modal {
					plugin_id: Arc::clone(&owner),
					title: None,
					tree,
				});
			}
		}
		if reply.close_overlay {
			surfaces.push_overlay(Overlay::Close {
				plugin_id: Arc::clone(&owner),
			});
		}
		if let Some(text) = reply.toast.filter(|text| !text.trim().is_empty()) {
			surfaces.push_overlay(Overlay::Toast {
				plugin_id: owner,
				tree: text_tree(clip(&text, MAX_TOAST_TEXT)),
				ttl: Duration::from_secs(5),
			});
		}
	}

	pub fn menu_item(
		&self,
		plugin_id: &str,
		menu: &str,
		key: &str,
		context: UiContext,
		detail: Option<ContextDetail>,
	) -> Result<()> {
		let hide_private = self.hides_private(plugin_id, context.guild_id);
		if hide_private && context.channel_id.is_some() {
			return Ok(());
		}
		self.instance(plugin_id)?
			.menu_item(
				menu.to_owned(),
				key.to_owned(),
				context,
				detail.map(|detail| sanitize_detail(detail, hide_private)),
			)
			.map_err(|error| Error::wasm(format!("{plugin_id}: {error}")))
	}

	pub fn render_views(&self, point: &str, context: ViewContext, detail: Option<ContextDetail>) -> bool {
		let Some(point) = mounts::context_point(point) else {
			return false;
		};
		let surfaces = self.runtime.mounts();
		let wanted = surfaces.request(point, context);
		let detail = detail.map(|detail| sanitize_detail(detail, false));
		for requested in &wanted {
			let plugin_id = &requested.plugin_id;
			let hide_private = self.hides_private(plugin_id, context.guild_id);
			let tree = match self.instance(plugin_id) {
				Ok(_) if hide_private => None,
				Ok(instance) => {
					match instance.render_view(point.to_owned(), context.to_api(None), detail.clone()) {
						Ok(tree) => tree.map(|tree| sanitize_view(&tree, &self.image_policy(plugin_id))),
						Err(error) => {
							tracing::debug!(target: "plugin", plugin = %plugin_id, "render-view skipped: {error}");
							None
						}
					}
				}
				Err(_) => None,
			};
			surfaces.store_requested_view(
				requested,
				point,
				context,
				tree.filter(|tree| !tree.roots.is_empty()),
			);
		}
		!wanted.is_empty()
	}

	pub fn pick_emoji(&self, emoji: &EmojiRef, channel_id: u64) {
		let overlay = match self.emoji_pick(emoji, channel_id) {
			EmojiPick::Default => return,
			EmojiPick::Insert { by, text } => Overlay::Insert {
				plugin_id: Arc::from(by),
				text,
			},
			EmojiPick::Blocked { by, reason } => Overlay::Toast {
				plugin_id: Arc::from(by),
				tree: text_tree(reason),
				ttl: Duration::from_secs(5),
			},
		};
		self.runtime.mounts().push_overlay(overlay);
	}

	pub fn emoji_pick(&self, emoji: &EmojiRef, channel_id: u64) -> EmojiPick {
		for entry in &self.hooks().emoji {
			match entry.instance.emoji_pick(emoji.clone(), channel_id) {
				Ok(PickVerdict::Default) => {}
				Ok(PickVerdict::Insert(text)) if !text.is_empty() && text.len() <= MAX_INSERT_BYTES => {
					return EmojiPick::Insert {
						by: entry.plugin.manifest.id.clone(),
						text,
					};
				}
				Ok(PickVerdict::Insert(_)) => {
					tracing::warn!(target: "plugin", plugin = %entry.id, "rejected emoji insert");
				}
				Ok(PickVerdict::Block(reason)) => {
					return EmojiPick::Blocked {
						by: entry.plugin.manifest.id.clone(),
						reason: clip(&reason, MAX_TOAST_TEXT),
					};
				}
				Err(error) => {
					tracing::debug!(target: "plugin", plugin = %entry.id, "emoji pick skipped: {error}");
				}
			}
		}
		EmojiPick::Default
	}

	pub fn before_send(&self, outgoing: OutgoingFull, skip: Option<&str>) -> FullSendDecision {
		let sets = self.hooks();
		let mut current = outgoing;
		let mut replaced_by: Option<String> = None;
		for entry in sets.send.iter().filter(|entry| Some(&*entry.id) != skip) {
			let step = if entry.ext_send {
				send_full_step(entry, &current)
			} else {
				send_plain_step(entry, &current)
			};
			match step {
				SendStep::Keep => {}
				SendStep::Cancel(reason) => {
					return FullSendDecision::Cancelled {
						by: entry.plugin.manifest.id.clone(),
						reason,
					};
				}
				SendStep::Replace(next) => {
					current = next;
					replaced_by = Some(entry.plugin.manifest.id.clone());
				}
			}
		}
		match replaced_by {
			Some(by) => FullSendDecision::Replaced { by, msg: current },
			None => FullSendDecision::Pass,
		}
	}

	pub fn dispatch_client_event(&self, event: ClientEvent, private_scope: bool) {
		let sets = self.hooks();
		let event = Arc::new(event);
		for entry in sets
			.events2
			.iter()
			.filter(|entry| entry.sees_client_event(&event, private_scope))
		{
			entry.instance.post_client_event(Arc::clone(&event));
		}
	}

	pub fn click_watchers(&self, click: &MessageClick) -> bool {
		let watchers = self.runtime.watches().message_targets(click);
		self.hooks()
			.message_click
			.iter()
			.any(|entry| entry.sees(click.message.guild_id) && watchers.contains(&entry.id))
	}

	pub fn channel_click_watchers(&self, click: &ChannelClick) -> bool {
		let watchers = self.runtime.watches().channel_targets(click);
		self.hooks()
			.channel_click
			.iter()
			.any(|entry| entry.sees(click.guild_id) && watchers.contains(&entry.id))
	}

	pub fn message_click(&self, click: &MessageClick) {
		let watchers = self.runtime.watches().message_targets(click);
		for entry in self
			.hooks()
			.message_click
			.iter()
			.filter(|entry| entry.sees(click.message.guild_id) && watchers.contains(&entry.id))
		{
			entry.instance.post_message_click(*click);
		}
	}

	pub fn channel_click(&self, click: &ChannelClick) {
		let watchers = self.runtime.watches().channel_targets(click);
		for entry in self
			.hooks()
			.channel_click
			.iter()
			.filter(|entry| entry.sees(click.guild_id) && watchers.contains(&entry.id))
		{
			entry.instance.post_channel_click(*click);
		}
	}

	pub fn declares(&self, hook: Hook) -> bool {
		let sets = self.hooks();
		!match hook {
			Hook::DraftChange => &sets.draft,
			Hook::AutocompleteEmoji => &sets.autocomplete,
			Hook::DecorateChannel => &sets.decorate,
			Hook::Notification => &sets.notification,
			Hook::MessageClick => &sets.message_click,
			Hook::ChannelClick => &sets.channel_click,
			Hook::LinkClick => &sets.link_click,
			Hook::DisplayUser => &sets.display,
			Hook::BeforeSend => &sets.send,
			Hook::Intercept => &sets.intercept,
		}
		.is_empty()
	}

	pub fn link_click(&self, url: &str, context: &UiContext) -> LinkVerdict {
		let sets = self.hooks();
		let started = Instant::now();
		let budget = self.runtime.config().timeouts.link_click;
		for entry in sets
			.link_click
			.iter()
			.filter(|entry| entry.sees(context.guild_id) && !entry.plugin.tripped(Breaker::LinkClick))
		{
			if started.elapsed() >= budget {
				break;
			}
			match entry.instance.link_click(url.to_owned(), context.clone()) {
				Ok(LinkVerdict::Pass) => entry.plugin.succeeded(Breaker::LinkClick),
				Ok(LinkVerdict::Consume) => return LinkVerdict::Consume,
				Ok(LinkVerdict::Open(target)) => {
					entry.plugin.succeeded(Breaker::LinkClick);
					if self.openable(&entry.id, &target) {
						return LinkVerdict::Open(target);
					}
					tracing::warn!(target: "plugin", plugin = %entry.id, "rejected link verdict");
				}
				Err(error) => entry.plugin.failed(Breaker::LinkClick, &error),
			}
		}
		LinkVerdict::Pass
	}

	fn openable(&self, plugin_id: &str, target: &str) -> bool {
		if target.len() > MAX_LINK_BYTES {
			return false;
		}
		Url::parse(target).is_ok_and(|parsed| {
			matches!(parsed.scheme(), "http" | "https")
				|| self
					.runtime
					.grants()
					.allows_url_scheme(plugin_id, parsed.scheme())
		})
	}

	pub fn draft_change(&self, channel_id: u64, guild_id: Option<u64>, text: &str) {
		for entry in self.hooks().draft.iter().filter(|entry| entry.sees(guild_id)) {
			entry.instance.post_draft_change(channel_id, text.to_owned());
		}
	}

	pub fn autocomplete_emoji(&self, query: &str, candidates: &[EmojiInfo]) -> Vec<u32> {
		let sets = self.hooks();
		if sets.autocomplete.is_empty() {
			return Vec::new();
		}
		let original: Arc<[EmojiInfo]> = candidates
			.iter()
			.take(MAX_AUTOCOMPLETE_CANDIDATES)
			.cloned()
			.collect();
		let mut shared = Arc::clone(&original);
		let mut chain: Vec<u32> = Vec::new();
		let started = Instant::now();
		let budget = self.runtime.config().timeouts.autocomplete_emoji;
		for entry in &sets.autocomplete {
			if started.elapsed() >= budget {
				break;
			}
			if entry.plugin.tripped(Breaker::Autocomplete) {
				continue;
			}
			match entry
				.instance
				.autocomplete_emoji(query.to_owned(), Arc::clone(&shared))
			{
				Ok(order) => {
					entry.plugin.succeeded(Breaker::Autocomplete);
					let mut seen = BTreeSet::new();
					let order: Vec<u32> = order
						.into_iter()
						.filter(|&index| (index as usize) < shared.len() && seen.insert(index))
						.collect();
					if order.is_empty() {
						continue;
					}
					chain = if chain.is_empty() {
						order
					} else {
						order.into_iter().map(|index| chain[index as usize]).collect()
					};
					shared = chain
						.iter()
						.map(|&index| original[index as usize].clone())
						.collect();
				}
				Err(error) => entry.plugin.failed(Breaker::Autocomplete, &error),
			}
		}
		chain
	}

	pub fn decorate_channels(&self, channels: &Arc<[ChannelDetail]>) -> Vec<ChannelBadges> {
		let sets = self.hooks();
		let mut answers = Vec::new();
		for entry in &sets.decorate {
			if entry.plugin.tripped(Breaker::Decorate) {
				continue;
			}
			let visible: Vec<usize> = (0..channels.len())
				.filter(|&index| entry.sees(channels[index].guild_id))
				.collect();
			if visible.is_empty() {
				continue;
			}
			let batch: Arc<[ChannelDetail]> = if visible.len() == channels.len() {
				Arc::clone(channels)
			} else {
				visible.iter().map(|&index| channels[index].clone()).collect()
			};
			match entry.instance.decorate_channels(batch) {
				Ok(badges) if badges.len() == visible.len() => {
					entry.plugin.succeeded(Breaker::Decorate);
					let mut per_channel = vec![Vec::new(); channels.len()];
					for (&index, list) in visible.iter().zip(badges) {
						per_channel[index] = sanitize_badges(list, &entry.images);
					}
					answers.push(ChannelBadges {
						plugin_id: Arc::clone(&entry.id),
						badges: per_channel,
					});
				}
				Ok(_) => {
					tracing::warn!(target: "plugin", plugin = %entry.id, "decorate-channel answered the wrong number of channels");
				}
				Err(error) => entry.plugin.failed(Breaker::Decorate, &error),
			}
		}
		answers
	}

	pub fn notification(&self, info: &NotificationInfo) -> NotificationVerdict {
		let sets = self.hooks();
		let started = Instant::now();
		let budget = self.runtime.config().timeouts.notification;
		for entry in sets
			.notification
			.iter()
			.filter(|entry| entry.sees(info.guild_id) && !entry.plugin.tripped(Breaker::Notification))
		{
			if started.elapsed() >= budget {
				break;
			}
			let outcome = entry.instance.notification(*info);
			match &outcome {
				Ok(_) => entry.plugin.succeeded(Breaker::Notification),
				Err(error) => entry.plugin.failed(Breaker::Notification, error),
			}
			match settle_notification(outcome) {
				NotificationVerdict::Allow => {}
				verdict => return verdict,
			}
		}
		NotificationVerdict::Allow
	}

	pub fn keybind(&self, plugin_id: &str, id: &str) {
		if self.runtime.grants().has(plugin_id, Permission::UiKeybinds)
			&& let Ok(instance) = self.instance(plugin_id)
		{
			instance.post_keybind(id.to_owned());
		}
	}

	pub fn badge_click(&self, plugin_id: &str, user_id: u64, key: &str) {
		if self.runtime.grants().has(plugin_id, Permission::UserDisplay)
			&& let Ok(instance) = self.instance(plugin_id)
		{
			instance.post_badge_click(user_id, key.to_owned());
		}
	}

	pub fn filter_message(&self, view: &Arc<MessageView>) -> bool {
		self.hooks()
			.filter
			.iter()
			.filter(|entry| entry.sees(view.guild_id))
			.all(|entry| match entry.instance.filter_message(Arc::clone(view)) {
				Ok(keep) => keep,
				Err(error) => {
					tracing::debug!(target: "plugin", plugin = %entry.id, "filter skipped: {error}");
					true
				}
			})
	}

	pub fn display_users(&self, users: &[(Option<u64>, User, Option<Presence>)]) -> Vec<Vec<DisplayAnswer>> {
		let sets = self.hooks();
		let mut answers: Vec<Vec<DisplayAnswer>> = vec![Vec::new(); users.len()];
		let everyone: Vec<usize> = (0..users.len()).collect();
		for entry in &sets.display {
			for chunk in everyone.chunks(DISPLAY_CHUNK) {
				if entry.plugin.tripped(Breaker::Display) {
					break;
				}
				if entry.ext_display {
					display_full_chunk(entry, users, chunk, &mut answers);
				} else {
					display_legacy_chunk(entry, users, chunk, &mut answers);
				}
			}
		}
		answers
	}

	pub fn ui_action(&self, plugin_id: &str, id: &str, payload: &str) -> Result<bool> {
		let instance = self.instance(plugin_id)?;
		let tree = instance
			.ui_action(id.to_owned(), payload.to_owned())
			.map_err(|error| Error::wasm(format!("{plugin_id}: {error}")))?;
		if tree.roots.is_empty() || validate_ui_tree(&tree).is_err() {
			return Ok(false);
		}
		let tree = sanitize_view(&upgrade_ui_tree(&tree), &ImagePolicy::new(true, BTreeSet::new()));
		Ok(self.runtime.slots().replace_owner_of(plugin_id, id, tree))
	}

	pub fn command(&self, name: &str, args: &str, channel_id: u64) -> std::result::Result<(), CommandError> {
		let owner = self.runtime.commands().owner(name).ok_or(CommandError::Unknown)?;
		let instance = self.instance(&owner).map_err(|_| CommandError::Unknown)?;
		match instance.command(name.to_owned(), args.to_owned(), channel_id)? {
			Ok(()) => Ok(()),
			Err(message) => Err(CommandError::Plugin(message)),
		}
	}

	pub fn settings_schema(&self, plugin_id: &str) -> Result<String> {
		let instance = self.instance(plugin_id)?;
		match instance.call(CallKind::SettingsSchema) {
			Ok(Output::Schema(schema)) => Ok(schema),
			Ok(_) => Err(Error::wasm("unexpected handler output")),
			Err(error) => Err(Error::wasm(format!("{plugin_id}: {error}"))),
		}
	}
}

struct InterceptWork {
	full: Arc<MessageFull>,
	meta: Arc<MessageMeta>,
	content: Arc<RichTree>,
	outcome: Intercepted,
	deleted: bool,
	embed_count: usize,
}

fn fallback_meta(full: &MessageFull) -> MessageMeta {
	MessageMeta {
		guild_id: full.view.guild_id,
		reply_to: None,
		flags: full.view.flags,
		latency_ms: None,
		author_color: None,
		pinned: false,
		edited_ms: None,
		nsfw_channel: false,
	}
}

fn code_block_count(tree: &RichTree) -> usize {
	tree.nodes
		.iter()
		.filter(|node| matches!(node, RichNode::Md(MdNode::CodeBlock(_))))
		.count()
}

impl InterceptWork {
	fn new(input: InterceptInput) -> Self {
		let InterceptInput { full, ast, meta } = input;
		Self {
			deleted: matches!(full.lifecycle, Lifecycle::Deleted(_)),
			embed_count: full.embeds.len(),
			content: Arc::new(md_to_rich(&ast)),
			meta: Arc::new(meta.unwrap_or_else(|| fallback_meta(&full))),
			full: Arc::new(full),
			outcome: Intercepted {
				ast,
				retain: false,
				patches: Vec::new(),
			},
		}
	}

	fn absorb(&mut self, entry: &HookEntry, sanitized: SanitizedPatch) {
		let SanitizedPatch {
			content: replaced,
			retain,
			patch,
		} = sanitized;
		if let Some(nodes) = replaced {
			let document = markdown::Document { nodes };
			Arc::make_mut(&mut self.full).view.content = document.plain_text();
			self.outcome.ast = document.nodes;
			self.content = Arc::new(md_to_rich(&self.outcome.ast));
		}
		if self.deleted && retain && entry.retain {
			self.outcome.retain = true;
		}
		self.outcome.patches.extend(patch);
	}
}

fn intercept_plain(entry: &HookEntry, work: &mut [InterceptWork], visible: &[usize]) {
	for &index in visible {
		let item = &mut work[index];
		match entry
			.instance
			.intercept(Arc::clone(&item.full), Arc::clone(&item.content))
		{
			Ok(patch) => {
				entry.plugin.succeeded(Breaker::Intercept);
				let sanitized = sanitize_patch(&entry.id, patch, item.embed_count, &entry.images);
				item.absorb(entry, sanitized);
			}
			Err(error) => {
				entry.plugin.failed(Breaker::Intercept, &error);
				if entry.plugin.tripped(Breaker::Intercept) {
					break;
				}
			}
		}
	}
}

fn intercept_ext(entry: &HookEntry, work: &mut [InterceptWork], visible: &[usize]) {
	let answers = match visible {
		[index] => {
			let item = &work[*index];
			entry
				.instance
				.intercept_ext(
					Arc::clone(&item.full),
					Arc::clone(&item.meta),
					Arc::clone(&item.content),
				)
				.map(|patch| vec![patch])
		}
		_ => {
			let batch: Arc<[InterceptItem]> = visible
				.iter()
				.map(|&index| {
					let item = &work[index];
					(
						MessageFull::clone(&item.full),
						*item.meta,
						RichTree::clone(&item.content),
					)
				})
				.collect();
			entry.instance.intercept_batch_ext(batch)
		}
	};
	match answers {
		Ok(patches) if patches.len() == visible.len() => {
			entry.plugin.succeeded(Breaker::Intercept);
			for (&index, patch) in visible.iter().zip(patches) {
				let item = &mut work[index];
				let sanitized = sanitize_patch_ext(
					&entry.id,
					patch,
					item.embed_count,
					&entry.images,
					&ExtContext {
						attachments: &item.full.attachments,
						code_blocks: code_block_count(&item.content),
					},
				);
				item.absorb(entry, sanitized);
			}
		}
		Ok(_) => {
			tracing::warn!(target: "plugin", plugin = %entry.id, "intercept-ext answered the wrong number of patches");
		}
		Err(error) => entry.plugin.failed(Breaker::Intercept, &error),
	}
}

fn display_full_chunk(
	entry: &HookEntry,
	users: &[(Option<u64>, User, Option<Presence>)],
	chunk: &[usize],
	answers: &mut [Vec<DisplayAnswer>],
) {
	let batch: Arc<[DisplayRequest]> = chunk
		.iter()
		.map(|&index| {
			let (guild_id, user, presence) = &users[index];
			(
				*guild_id,
				user.clone(),
				presence.clone().filter(|_| entry.presence),
			)
		})
		.collect();
	match entry.instance.display_users_full(batch) {
		Ok(overrides) => {
			entry.plugin.succeeded(Breaker::Display);
			for (&index, over) in chunk.iter().zip(overrides) {
				if let Some(over) = over.and_then(|over| sanitize_display_full(over, &entry.images)) {
					answers[index].push(DisplayAnswer::Full(Arc::clone(&entry.id), over));
				}
			}
		}
		Err(error) => entry.plugin.failed(Breaker::Display, &error),
	}
}

fn display_legacy_chunk(
	entry: &HookEntry,
	users: &[(Option<u64>, User, Option<Presence>)],
	chunk: &[usize],
	answers: &mut [Vec<DisplayAnswer>],
) {
	let batch: Arc<[(Option<u64>, User)]> = chunk
		.iter()
		.map(|&index| {
			let (guild_id, user, _) = &users[index];
			(*guild_id, user.clone())
		})
		.collect();
	match entry.instance.display_users(batch) {
		Ok(overrides) => {
			entry.plugin.succeeded(Breaker::Display);
			for (&index, over) in chunk.iter().zip(overrides) {
				if let Some(over) = over {
					answers[index].push(DisplayAnswer::Legacy(sanitize_override(over)));
				}
			}
		}
		Err(error) => entry.plugin.failed(Breaker::Display, &error),
	}
}

enum SendStep {
	Keep,
	Cancel(String),
	Replace(OutgoingFull),
}

fn send_full_step(entry: &HookEntry, current: &OutgoingFull) -> SendStep {
	if entry.plugin.tripped(Breaker::Send) {
		return SendStep::Keep;
	}
	match entry.instance.before_send_full(Arc::new(current.clone())) {
		Ok(decision) => {
			entry.plugin.succeeded(Breaker::Send);
			match decision {
				SendDecisionFull::Pass => SendStep::Keep,
				SendDecisionFull::Cancel(reason) => SendStep::Cancel(clip(&reason, MAX_TOAST_TEXT)),
				SendDecisionFull::Replace(message) => match checked_replacement(current, message) {
					Some(message) if message != *current => SendStep::Replace(message),
					Some(_) => SendStep::Keep,
					None => {
						tracing::warn!(target: "plugin", plugin = %entry.id, "rejected rewritten message");
						SendStep::Keep
					}
				},
			}
		}
		Err(error) => {
			entry.plugin.failed(Breaker::Send, &error);
			SendStep::Keep
		}
	}
}

fn send_plain_step(entry: &HookEntry, current: &OutgoingFull) -> SendStep {
	if current.content.is_empty() || entry.plugin.tripped(Breaker::Send) {
		return SendStep::Keep;
	}
	let message = Arc::new(OutgoingMessage {
		channel_id: current.channel_id,
		content: current.content.clone(),
		reply_to: current.reply_to,
	});
	match entry.instance.before_send(message) {
		Ok(GuestDecision::Pass) => SendStep::Keep,
		Ok(GuestDecision::Cancel(reason)) => SendStep::Cancel(clip(&reason, MAX_TOAST_TEXT)),
		Ok(GuestDecision::Replace(message)) => {
			let keeps_target =
				message.channel_id == current.channel_id && message.reply_to == current.reply_to;
			if keeps_target && !message.content.is_empty() && message.content.len() <= MAX_OUTGOING_BYTES {
				SendStep::Replace(OutgoingFull {
					content: message.content,
					..current.clone()
				})
			} else {
				tracing::warn!(target: "plugin", plugin = %entry.id, "rejected rewritten message");
				SendStep::Keep
			}
		}
		Err(error) => {
			tracing::debug!(target: "plugin", plugin = %entry.id, "before-send skipped: {error}");
			SendStep::Keep
		}
	}
}

fn file_name(name: &str) -> Option<String> {
	let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
	let cleaned = clip_line(base, MAX_ATTACHMENT_NAME);
	(!cleaned.trim().is_empty()).then_some(cleaned)
}

pub(crate) fn checked_replacement(before: &OutgoingFull, mut after: OutgoingFull) -> Option<OutgoingFull> {
	if after.channel_id != before.channel_id
		|| after.reply_to != before.reply_to
		|| !keeps_attachments(before, &after)
	{
		return None;
	}
	after.content = clip(&after.content, MAX_OUTGOING_BYTES);
	for attachment in &mut after.attachments {
		let original = before
			.attachments
			.iter()
			.find(|original| original.handle == attachment.handle)?;
		attachment.size = original.size;
		attachment.content_type.clone_from(&original.content_type);
		attachment.name = file_name(&attachment.name).unwrap_or_else(|| original.name.clone());
	}
	let mut seen = BTreeSet::new();
	after.stickers.retain(|sticker| seen.insert(*sticker));
	after.stickers.truncate(MAX_STICKERS);
	let has_body =
		!after.content.trim().is_empty() || !after.attachments.is_empty() || !after.stickers.is_empty();
	has_body.then_some(after)
}

pub(crate) fn settle_notification(
	outcome: std::result::Result<NotificationVerdict, CallError>,
) -> NotificationVerdict {
	match outcome {
		Ok(NotificationVerdict::Replace(over)) => {
			sanitize_notification(over).map_or(NotificationVerdict::Allow, NotificationVerdict::Replace)
		}
		Ok(verdict) => verdict,
		Err(_) => NotificationVerdict::Allow,
	}
}

fn sanitize_notification(over: NotificationOverride) -> Option<NotificationOverride> {
	let title = clip_line(&over.title, MAX_NOTIFICATION_TITLE);
	if title.trim().is_empty() {
		return None;
	}
	let body = clip(&over.body, MAX_NOTIFICATION_BODY)
		.chars()
		.filter(|character| *character == '\n' || !character.is_control())
		.collect();
	Some(NotificationOverride {
		title,
		body,
		sound: over.sound,
	})
}

fn sanitize_detail(detail: ContextDetail, hide_private: bool) -> ContextDetail {
	ContextDetail {
		selection: detail
			.selection
			.filter(|_| !hide_private)
			.map(|selection| clip(&selection, MAX_SELECTION_BYTES)),
		image_url: detail.image_url.filter(|url| !hide_private && web_url(url)),
		attachment_id: detail.attachment_id.filter(|_| !hide_private),
		..detail
	}
}

fn keeps_attachments(before: &OutgoingFull, after: &OutgoingFull) -> bool {
	let handles = |message: &OutgoingFull| {
		let mut handles: Vec<u64> = message
			.attachments
			.iter()
			.map(|attachment| attachment.handle)
			.collect();
		handles.sort_unstable();
		handles
	};
	handles(before) == handles(after)
}

fn render_chain<'a>(
	plugins: impl Iterator<Item = &'a HookEntry>,
	view: &Arc<MessageView>,
	ast: &[Node],
) -> Option<Vec<Node>> {
	let mut plugins = plugins.peekable();
	plugins.peek()?;
	let mut tree = Arc::new(md_to_tree(ast));
	let mut changed = false;
	for entry in plugins {
		match entry.instance.render_message(Arc::clone(view), Arc::clone(&tree)) {
			Ok(Some(output)) if tree_to_md(&output).is_some() => {
				tree = Arc::new(output);
				changed = true;
			}
			Ok(Some(_)) => {
				tracing::warn!(target: "plugin", plugin = %entry.id, "invalid markdown tree ignored");
			}
			Ok(None) => {}
			Err(error) => {
				tracing::debug!(target: "plugin", plugin = %entry.id, "render skipped: {error}");
			}
		}
	}
	changed.then(|| tree_to_md(&tree)).flatten()
}

fn text_tree(text: String) -> ViewTree {
	ViewTree {
		nodes: vec![ViewNode {
			key: None,
			style: None,
			kind: NodeKind::Text(TextNode {
				value: text,
				size: TextSize::Sm,
				weight: Weight::Normal,
				tone: Tone::Normal,
				color: None,
				mono: false,
				italic: false,
			}),
		}],
		roots: vec![0],
	}
}

fn sanitize_override(mut over: DisplayOverride) -> DisplayOverride {
	over.name = over
		.name
		.map(|name| {
			name.chars()
				.filter(|ch| !ch.is_control())
				.take(MAX_DISPLAY_NAME_CHARS)
				.collect()
		})
		.filter(|name: &String| !name.trim().is_empty());
	over.badge = over
		.badge
		.map(|badge| {
			badge
				.chars()
				.filter(|ch| !ch.is_control())
				.take(MAX_BADGE_CHARS)
				.collect()
		})
		.filter(|badge: &String| !badge.trim().is_empty());
	over.color = over.color.filter(|color| is_hex_color(color));
	over
}

fn is_hex_color(color: &str) -> bool {
	color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
