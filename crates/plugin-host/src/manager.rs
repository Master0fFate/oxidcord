use std::path::{Path, PathBuf};
use std::sync::Arc;

use markdown::Node;
use parking_lot::{Mutex, RwLock};
use semver::Version;

use crate::api::{
	DisplayOverride, Event, MessageView, OutgoingMessage, SendDecision as GuestDecision, User, VoiceInfo,
};
use crate::convert::{md_to_tree, tree_to_md, validate_ui_tree};
use crate::error::{Error, Result};
use crate::instance::{CallError, CallKind, Health, Output, PluginInstance, PluginState};
use crate::manifest::{Manifest, PermissionRequest};
use crate::package::{Package, Trust, discover};
use crate::permission::Permission;
use crate::runtime::Runtime;
use crate::slots::{CommandEntry, Slots};

const MAX_OUTGOING_BYTES: usize = 4_000;
const MAX_DISPLAY_NAME_CHARS: usize = 80;
const MAX_BADGE_CHARS: usize = 16;

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
}

impl LoadedPlugin {
	fn instance(&self) -> Option<Arc<PluginInstance>> {
		self.instance.lock().clone()
	}

	fn is_active(&self) -> bool {
		matches!(self.health.state(), PluginState::Loaded | PluginState::Crashed)
	}
}

pub struct PluginManager {
	runtime: Runtime,
	plugins: RwLock<Vec<Arc<LoadedPlugin>>>,
	safe_mode: Mutex<bool>,
}

impl PluginManager {
	pub fn new(runtime: Runtime) -> Self {
		Self {
			runtime,
			plugins: RwLock::new(Vec::new()),
			safe_mode: Mutex::new(false),
		}
	}

	pub fn runtime(&self) -> &Runtime {
		&self.runtime
	}

	pub fn slots(&self) -> &Arc<Slots> {
		self.runtime.slots()
	}

	pub fn commands(&self) -> Vec<(String, CommandEntry)> {
		self.runtime.commands().list()
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
		self.start(&plugin)?;
		Ok(id)
	}

	pub fn unload(&self, id: &str) -> Result<PathBuf> {
		let plugin = self.find(id)?;
		self.retire(&plugin);
		self.plugins
			.write()
			.retain(|loaded| !Arc::ptr_eq(loaded, &plugin));
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
		let grants = self.runtime.grants();
		self.plugins.read().iter().any(|plugin| {
			plugin.is_active() && plugin.instance().is_some() && grants.has(&plugin.manifest.id, permission)
		})
	}

	pub fn has_instances(&self) -> bool {
		self.plugins
			.read()
			.iter()
			.any(|plugin| plugin.is_active() && plugin.instance().is_some())
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
			health: Arc::new(Health::new(state)),
			instance: Mutex::new(None),
			load_error,
			component,
		}))
	}

	fn start(&self, plugin: &Arc<LoadedPlugin>) -> Result<()> {
		let Some(component) = &plugin.component else {
			return Ok(());
		};
		if plugin.health.state() == PluginState::Incompatible {
			return Ok(());
		}
		let instance = PluginInstance::new(
			Arc::from(plugin.manifest.id.as_str()),
			Arc::clone(self.runtime.shared()),
			component,
			Arc::clone(&plugin.health),
		)?;
		*plugin.instance.lock() = Some(Arc::clone(&instance));
		plugin.health.set_state(PluginState::Loaded);
		instance
			.start()
			.map_err(|error| Error::wasm(format!("{}: {error}", plugin.manifest.id)))
	}

	fn retire(&self, plugin: &Arc<LoadedPlugin>) {
		if let Some(instance) = plugin.instance.lock().take() {
			instance.shutdown();
		}
		self.runtime.slots().clear_plugin(&plugin.manifest.id);
		self.runtime.commands().clear_plugin(&plugin.manifest.id);
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
					granted: grant.permissions.into_iter().collect(),
					network_hosts: plugin.manifest.network_hosts.clone(),
					granted_hosts: grant.network_hosts.into_iter().collect(),
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

	fn active(&self, permission: Permission) -> Vec<(Arc<LoadedPlugin>, Arc<PluginInstance>)> {
		let grants = self.runtime.grants();
		self.plugins
			.read()
			.iter()
			.filter(|plugin| plugin.is_active())
			.filter(|plugin| grants.has(&plugin.manifest.id, permission))
			.filter_map(|plugin| Some((Arc::clone(plugin), plugin.instance()?)))
			.collect()
	}

	fn active_for_message(
		&self,
		permission: Permission,
		guild_id: Option<u64>,
	) -> Vec<(Arc<LoadedPlugin>, Arc<PluginInstance>)> {
		let grants = self.runtime.grants();
		let mut plugins = self.active(permission);
		if guild_id.is_none() {
			plugins.retain(|(plugin, _)| grants.has(&plugin.manifest.id, Permission::ContextPrivate));
		}
		plugins
	}

	pub fn dispatch_event(&self, event: &Event) {
		let grants = self.runtime.grants();
		let targets: Vec<_> = self
			.plugins
			.read()
			.iter()
			.filter(|plugin| plugin.is_active())
			.filter(|plugin| event_visible(grants, &plugin.manifest.id, event))
			.filter_map(|plugin| plugin.instance())
			.collect();
		for instance in targets {
			instance.post_event(event.clone());
		}
	}

	pub fn dispatch_voice_changed(&self, info: VoiceInfo) {
		self.dispatch_event(&Event::VoiceChanged(info));
	}

	pub fn render_message(&self, view: &MessageView, ast: Vec<Node>) -> Vec<Node> {
		let plugins = self.active_for_message(Permission::MessageRender, view.guild_id);
		if plugins.is_empty() {
			return ast;
		}
		let mut tree = md_to_tree(&ast);
		for (plugin, instance) in plugins {
			match instance.render_message(view.clone(), tree.clone()) {
				Ok(Some(output)) if tree_to_md(&output).is_some() => tree = output,
				Ok(Some(_)) => {
					tracing::warn!(target: "plugin", plugin = %plugin.manifest.id, "invalid markdown tree ignored");
				}
				Ok(None) => {}
				Err(error) => {
					tracing::debug!(target: "plugin", plugin = %plugin.manifest.id, "render skipped: {error}");
				}
			}
		}
		tree_to_md(&tree).unwrap_or(ast)
	}

	pub fn before_send(&self, outgoing: OutgoingMessage) -> SendDecision {
		let mut current = outgoing;
		let mut replaced_by: Option<String> = None;
		for (plugin, instance) in self.active(Permission::MessageSend) {
			let decision = match instance.before_send(current.clone()) {
				Ok(decision) => decision,
				Err(error) => {
					tracing::debug!(target: "plugin", plugin = %plugin.manifest.id, "before-send skipped: {error}");
					continue;
				}
			};
			match decision {
				GuestDecision::Pass => {}
				GuestDecision::Cancel(reason) => {
					return SendDecision::Cancelled {
						by: plugin.manifest.id.clone(),
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
						current = message;
						replaced_by = Some(plugin.manifest.id.clone());
					} else {
						tracing::warn!(target: "plugin", plugin = %plugin.manifest.id, "rejected rewritten message");
					}
				}
			}
		}
		match replaced_by {
			Some(by) => SendDecision::Replaced { by, msg: current },
			None => SendDecision::Pass,
		}
	}

	pub fn filter_message(&self, view: &MessageView) -> bool {
		self.active_for_message(Permission::MessageFilter, view.guild_id)
			.into_iter()
			.all(|(plugin, instance)| match instance.filter_message(view.clone()) {
				Ok(keep) => keep,
				Err(error) => {
					tracing::debug!(target: "plugin", plugin = %plugin.manifest.id, "filter skipped: {error}");
					true
				}
			})
	}

	pub fn display_user(&self, guild_id: Option<u64>, user: &User) -> Option<DisplayOverride> {
		for (plugin, instance) in self.active(Permission::UserDisplay) {
			match instance.display_user(guild_id, user.clone()) {
				Ok(Some(over)) => return Some(sanitize_override(over)),
				Ok(None) => {}
				Err(error) => {
					tracing::debug!(target: "plugin", plugin = %plugin.manifest.id, "display skipped: {error}");
				}
			}
		}
		None
	}

	pub fn ui_action(&self, plugin_id: &str, id: &str, payload: &str) -> Result<bool> {
		let instance = self.instance(plugin_id)?;
		let tree = instance
			.ui_action(id.to_owned(), payload.to_owned())
			.map_err(|error| Error::wasm(format!("{plugin_id}: {error}")))?;
		if tree.roots.is_empty() || validate_ui_tree(&tree).is_err() {
			return Ok(false);
		}
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

fn event_visible(grants: &crate::grants::Grants, id: &str, event: &Event) -> bool {
	let timeline = |guild_id: Option<u64>| {
		grants.has(id, Permission::ContextTimeline)
			&& (guild_id.is_some() || grants.has(id, Permission::ContextPrivate))
	};
	match event {
		Event::MessageCreated(message) | Event::MessageUpdated(message) => timeline(message.guild_id),
		Event::MessageDeleted(message) => timeline(message.guild_id),
		Event::ReactionAdded(reaction) => timeline(reaction.message.guild_id),
		Event::Typing(_) => grants.has(id, Permission::ContextTimeline),
		Event::ChannelSwitched(switch) => {
			grants.has(id, Permission::ContextBasic)
				&& (switch.guild_id.is_some() || grants.has(id, Permission::ContextPrivate))
		}
		Event::VoiceChanged(info) => {
			grants.has(id, Permission::ContextVoice)
				&& (info.guild_id.is_some() || grants.has(id, Permission::ContextPrivate))
		}
		Event::SettingChanged(_) | Event::Connection(_) => true,
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
