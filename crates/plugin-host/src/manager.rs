use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use markdown::Node;
use parking_lot::{Mutex, RwLock};
use semver::Version;

use crate::api::model::{
	EmojiRef, Lifecycle, MessageFull, NodeKind, PickVerdict, TextNode, TextSize, Tone, UiContext, ViewEvent,
	ViewNode, ViewReply, ViewTree, Weight,
};
use crate::api::{
	DisplayOverride, Event, MessageView, OutgoingMessage, SendDecision as GuestDecision, User, VoiceInfo,
};
use crate::convert::{
	ImagePolicy, SanitizedPatch, clip, md_to_rich, md_to_tree, sanitize_patch, sanitize_view, tree_to_md,
	upgrade_ui_tree, validate_ui_tree,
};
use crate::error::{Error, Result};
use crate::instance::{CallError, CallKind, Health, Output, PluginInstance, PluginState};
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
const INTERCEPT_BREAKER: u32 = 3;
const MAX_INSERT_BYTES: usize = 2_000;
const MAX_TOAST_TEXT: usize = 200;
const DISPLAY_CHUNK: usize = 50;

pub struct Intercepted {
	pub ast: Vec<Node>,
	pub retain: bool,
	pub patches: Vec<PluginPatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmojiPick {
	Default,
	Insert { by: String, text: String },
	Blocked { by: String, reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SendDecision {
	Pass,
	Replaced { by: String, msg: OutgoingMessage },
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

struct LoadedPlugin {
	manifest: Arc<Manifest>,
	dir: PathBuf,
	trust: Trust,
	health: Arc<Health>,
	instance: Mutex<Option<Arc<PluginInstance>>>,
	load_error: Option<String>,
	component: Option<wasmtime::component::Component>,
	intercept_timeouts: AtomicU32,
}

impl LoadedPlugin {
	fn instance(&self) -> Option<Arc<PluginInstance>> {
		self.instance.lock().clone()
	}

	fn is_active(&self) -> bool {
		self.health.state().is_active()
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
			};
			sets.hidden_channels |= has(Permission::ChannelsHidden);
			sets.hidden_typing |= has(Permission::TypingHidden);
			let targets = [
				(Permission::MessageRender, true, &mut sets.render),
				(Permission::MessageFilter, true, &mut sets.filter),
				(Permission::MessageIntercept, ext, &mut sets.intercept),
				(Permission::MessageSend, true, &mut sets.send),
				(Permission::UserDisplay, true, &mut sets.display),
				(Permission::Composer, ext, &mut sets.emoji),
			];
			for (permission, capable, set) in targets {
				if capable && has(permission) {
					set.push(entry.clone());
				}
			}
			sets.events.push(entry);
		}
		sets
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
			instance: Mutex::new(None),
			load_error,
			component,
			intercept_timeouts: AtomicU32::new(0),
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
		)?;
		self.runtime.timers().attach(&id, Arc::downgrade(&instance));
		*plugin.instance.lock() = Some(Arc::clone(&instance));
		plugin.health.set_state(PluginState::Loaded);
		self.rehook();
		instance
			.start()
			.map_err(|error| Error::wasm(format!("{}: {error}", plugin.manifest.id)))
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
					error: plugin.load_error.clone().or_else(|| plugin.health.last_error()),
					crashes: plugin.health.total_crashes(),
				}
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
		let sets = self.hooks();
		let event = Arc::new(event);
		for entry in sets.events.iter().filter(|entry| entry.sees_event(&event)) {
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

	pub fn intercept(&self, full: MessageFull, ast: Vec<Node>) -> Intercepted {
		let sets = self.hooks();
		let guild_id = full.view.guild_id;
		let mut plugins = sets
			.intercept
			.iter()
			.filter(|entry| entry.sees(guild_id))
			.peekable();
		let mut outcome = Intercepted {
			ast,
			retain: false,
			patches: Vec::new(),
		};
		if plugins.peek().is_none() {
			return outcome;
		}
		let deleted = matches!(full.lifecycle, Lifecycle::Deleted(_));
		let embed_count = full.embeds.len();
		let started = Instant::now();
		let mut content = Arc::new(md_to_rich(&outcome.ast));
		let mut shared = Arc::new(full);
		for entry in plugins {
			if started.elapsed() >= INTERCEPT_BUDGET {
				tracing::debug!(target: "plugin", "intercept budget spent, remaining plugins skipped");
				break;
			}
			if entry.plugin.intercept_timeouts.load(Ordering::Relaxed) >= INTERCEPT_BREAKER {
				continue;
			}
			let patch = match entry
				.instance
				.intercept(Arc::clone(&shared), Arc::clone(&content))
			{
				Ok(patch) => {
					entry.plugin.intercept_timeouts.store(0, Ordering::Relaxed);
					patch
				}
				Err(error) => {
					if matches!(error, CallError::Timeout)
						&& entry.plugin.intercept_timeouts.fetch_add(1, Ordering::Relaxed) + 1
							== INTERCEPT_BREAKER
					{
						tracing::warn!(target: "plugin", plugin = %entry.id, "intercept-message disabled after repeated timeouts");
					}
					tracing::debug!(target: "plugin", plugin = %entry.id, "intercept skipped: {error}");
					continue;
				}
			};
			let SanitizedPatch {
				content: replaced,
				retain,
				patch,
			} = sanitize_patch(&entry.id, patch, embed_count, &entry.images);
			if let Some(nodes) = replaced {
				let document = markdown::Document { nodes };
				Arc::make_mut(&mut shared).view.content = document.plain_text();
				outcome.ast = document.nodes;
				content = Arc::new(md_to_rich(&outcome.ast));
			}
			if deleted && retain && entry.retain {
				outcome.retain = true;
			}
			outcome.patches.extend(patch);
		}
		outcome
	}

	pub fn store_patches(&self, message_id: u64, patches: Vec<PluginPatch>) {
		self.runtime.patches().set(message_id, patches);
	}

	pub fn view_event(&self, plugin_id: &str, event: ViewEvent) -> Result<()> {
		let instance = self.instance(plugin_id)?;
		let point = event.point.clone();
		let mount = event.mount.clone();
		let context = ViewContext {
			guild_id: event.context.guild_id,
			channel_id: event.context.channel_id,
			message_id: event.context.message_id,
			user_id: event.context.user_id,
		};
		let reply = instance
			.view_event(event)
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
				surfaces.store_view(&owner, point, context, Some(tree));
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

	pub fn menu_item(&self, plugin_id: &str, menu: &str, key: &str, context: UiContext) -> Result<()> {
		self.instance(plugin_id)?
			.menu_item(menu.to_owned(), key.to_owned(), context)
			.map_err(|error| Error::wasm(format!("{plugin_id}: {error}")))
	}

	pub fn render_views(&self, point: &str, context: ViewContext) -> bool {
		let Some(point) = mounts::context_point(point) else {
			return false;
		};
		let surfaces = self.runtime.mounts();
		let wanted = surfaces.request(point, context);
		for requested in &wanted {
			let plugin_id = &requested.plugin_id;
			let tree = match self.instance(plugin_id) {
				Ok(instance) => match instance.render_view(point.to_owned(), context.to_api(None)) {
					Ok(tree) => tree.map(|tree| sanitize_view(&tree, &self.image_policy(plugin_id))),
					Err(error) => {
						tracing::debug!(target: "plugin", plugin = %plugin_id, "render-view skipped: {error}");
						None
					}
				},
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

	pub fn before_send(&self, outgoing: OutgoingMessage, skip: Option<&str>) -> SendDecision {
		let sets = self.hooks();
		let mut current = Arc::new(outgoing);
		let mut replaced_by: Option<String> = None;
		for entry in sets.send.iter().filter(|entry| Some(&*entry.id) != skip) {
			let decision = match entry.instance.before_send(Arc::clone(&current)) {
				Ok(decision) => decision,
				Err(error) => {
					tracing::debug!(target: "plugin", plugin = %entry.id, "before-send skipped: {error}");
					continue;
				}
			};
			match decision {
				GuestDecision::Pass => {}
				GuestDecision::Cancel(reason) => {
					return SendDecision::Cancelled {
						by: entry.plugin.manifest.id.clone(),
						reason,
					};
				}
				GuestDecision::Replace(message) => {
					let keeps_target =
						message.channel_id == current.channel_id && message.reply_to == current.reply_to;
					if keeps_target
						&& !message.content.is_empty()
						&& message.content.len() <= MAX_OUTGOING_BYTES
					{
						current = Arc::new(message);
						replaced_by = Some(entry.plugin.manifest.id.clone());
					} else {
						tracing::warn!(target: "plugin", plugin = %entry.id, "rejected rewritten message");
					}
				}
			}
		}
		match replaced_by {
			Some(by) => SendDecision::Replaced {
				by,
				msg: Arc::try_unwrap(current).unwrap_or_else(|shared| (*shared).clone()),
			},
			None => SendDecision::Pass,
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

	pub fn display_users(&self, users: &[(Option<u64>, User)]) -> Vec<Option<DisplayOverride>> {
		let sets = self.hooks();
		let mut resolved: Vec<Option<DisplayOverride>> = vec![None; users.len()];
		for entry in &sets.display {
			let pending: Vec<usize> = (0..users.len())
				.filter(|&index| resolved[index].is_none())
				.collect();
			for chunk in pending.chunks(DISPLAY_CHUNK) {
				let batch: Arc<[(Option<u64>, User)]> =
					chunk.iter().map(|&index| users[index].clone()).collect();
				match entry.instance.display_users(batch) {
					Ok(overrides) => {
						for (&index, over) in chunk.iter().zip(overrides) {
							resolved[index] = over.map(sanitize_override);
						}
					}
					Err(error) => {
						tracing::debug!(target: "plugin", plugin = %entry.id, "display skipped: {error}");
					}
				}
			}
			if resolved.iter().all(Option::is_some) {
				break;
			}
		}
		resolved
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
