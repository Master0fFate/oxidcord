use std::collections::HashSet;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use plugin_host::api::{
	ChannelSwitch, DisplayOverride, Event as PluginEvent, MessageRef, OutgoingMessage, ReactionEvent,
	TypingEvent,
};
use plugin_host::convert::user_view;
use plugin_host::{PACKAGE_EXTENSION, SendDecision};
use protocol::{DispatchEvent, Message, MessageUpdate, Nonce, PartialEmoji, Snowflake, User};
use rest::UploadFile;
use state::{Changes, Event, LocalEvent, MemberSlot, RenderedMessage, UserOverride, apply_update};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::{Core, Internal};
use crate::broker::{view_of, voice_info};
use crate::catalog::{download_plugin, download_theme, fetch_catalog, fetch_theme_catalog, write_theme};
use crate::command::Command;
use crate::plugins::{PluginFlags, PluginHub};
use crate::render::{RenderJob, RenderResult};

const OVERRIDE_DEBOUNCE: Duration = Duration::from_secs(2);
const OVERRIDE_BATCH: usize = 200;
const OVERRIDE_AUTHOR_WINDOW: usize = 100;

type OverrideEntry = (Option<Snowflake>, Snowflake, Option<UserOverride>);

pub(super) type PluginJob = Box<dyn FnOnce() + Send>;

pub(super) fn spawn_plugin_jobs(runtime: &Handle) -> mpsc::UnboundedSender<PluginJob> {
	let (sender, mut jobs) = mpsc::unbounded_channel::<PluginJob>();
	runtime.spawn(async move {
		while let Some(job) = jobs.recv().await {
			if let Err(error) = tokio::task::spawn_blocking(job).await {
				tracing::warn!(target: "plugin", %error, "plugin job panicked");
			}
		}
	});
	sender
}

fn emoji_text(emoji: &PartialEmoji) -> String {
	match (&emoji.name, emoji.id) {
		(Some(name), Some(id)) => format!("{name}:{id}"),
		(Some(name), None) => name.clone(),
		(None, Some(id)) => id.to_string(),
		(None, None) => String::new(),
	}
}

fn to_override(over: DisplayOverride) -> Option<UserOverride> {
	let color = over
		.color
		.as_deref()
		.and_then(|color| u32::from_str_radix(color.trim_start_matches('#'), 16).ok());
	(over.name.is_some() || color.is_some() || over.badge.is_some()).then_some(UserOverride {
		name: over.name,
		color,
		badge: over.badge,
	})
}

fn uncached_update(update: &MessageUpdate) -> Option<Message> {
	update.content.as_ref()?;
	update.author.as_ref()?;
	let mut message = Message {
		id: update.id,
		channel_id: update.channel_id,
		guild_id: update.guild_id,
		timestamp: update.timestamp,
		..Message::default()
	};
	apply_update(&mut message, update);
	Some(message)
}

fn download_name() -> String {
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_nanos());
	format!("catalog-{nanos:x}.{PACKAGE_EXTENSION}")
}

async fn install_from_catalog(hub: Arc<PluginHub>, id: &str) -> Result<()> {
	let entry = hub
		.catalog()
		.and_then(|catalog| catalog.plugins.iter().find(|entry| entry.id == id).cloned())
		.context("plugin is not in the catalog")?;
	let archive = hub.store.paths.cache_dir().join(download_name());
	let downloaded = download_plugin(&entry, &archive).await;
	let installed = match downloaded {
		Ok(()) => {
			let target = archive.clone();
			tokio::task::spawn_blocking(move || hub.install(&target, Some(&entry)))
				.await
				.context("install task failed")?
		}
		Err(error) => Err(error),
	};
	let _ = fs::remove_file(&archive);
	installed
}

async fn install_theme_from_catalog(hub: Arc<PluginHub>, id: &str) -> Result<()> {
	let entry = hub
		.theme_catalog()
		.and_then(|catalog| catalog.themes.iter().find(|entry| entry.id == id).cloned())
		.context("theme is not in the catalog")?;
	let bytes = download_theme(&entry).await?;
	let themes_dir = hub.store.paths.themes_dir();
	tokio::task::spawn_blocking(move || write_theme(&themes_dir, &entry, &bytes))
		.await
		.context("install task failed")?
}

impl Core {
	pub(super) fn start_plugins(&self) {
		let hub = Arc::clone(&self.hub);
		let internal = self.internal.clone();
		let safe_mode = self.safe_mode;
		self.queue_plugin_job(move || {
			hub.load_all(safe_mode);
			let _ = internal.send(Internal::PluginsChanged);
		});
	}

	fn queue_plugin_job(&self, job: impl FnOnce() + Send + 'static) {
		if self.plugin_jobs.send(Box::new(job)).is_err() {
			tracing::warn!(target: "plugin", "plugin job queue is closed");
		}
	}

	pub(super) fn on_plugins_changed(&mut self) {
		let flags = self.hub.flags();
		let logs_changed = flags.logs != self.flags.logs;
		self.flags = flags;
		if logs_changed {
			self.apply_local(LocalEvent::KeepDeleted(flags.logs));
		}
		self.epoch += 1;
		self.render_worker.set_epoch(self.epoch);
		self.apply_local(LocalEvent::UserOverridesCleared);
		let selected = self.state.read().selected();
		if let Some(selected) = selected {
			self.rerender_stale(selected.channel_id);
		}
		self.schedule_overrides(Duration::ZERO);
		self.notifier.push(Changes {
			plugins: true,
			plugin_slots: true,
			..Changes::default()
		});
	}

	pub(super) fn on_slots_changed(&mut self) {
		self.notifier.push(Changes {
			plugin_slots: true,
			plugins: self.hub.last_error().is_some(),
			..Changes::default()
		});
	}

	pub(super) fn on_catalog_changed(&mut self) {
		self.notifier.push(Changes {
			plugins: true,
			..Changes::default()
		});
	}

	pub(super) fn render_messages(&mut self, messages: Vec<Arc<Message>>) {
		let PluginFlags { filter, .. } = self.flags;
		let revision = self.epoch;
		let parsed: Vec<(Arc<Message>, RenderedMessage)> = messages
			.into_iter()
			.map(|message| {
				let rendered = RenderedMessage::plain(&message.content, filter, revision);
				(message, rendered)
			})
			.collect();
		let mut jobs = Vec::new();
		let mut state = self.state.write();
		for (message, mut rendered) in parsed {
			let channel_id = message.channel_id;
			let deleted = state
				.messages(channel_id)
				.is_some_and(|cache| cache.is_deleted(message.id));
			if deleted {
				rendered.hidden = true;
				rendered.deleted = true;
			}
			let rendered = Arc::new(rendered);
			if self.flags.renders() {
				jobs.push(RenderJob {
					guild_id: message.guild_id.or_else(|| state.channel_guild(channel_id)),
					message: Arc::clone(&message),
					epoch: revision,
					deleted,
				});
			}
			state.apply(&Event::Local(LocalEvent::MessageRendered {
				channel_id,
				id: message.id,
				source: message,
				rendered,
			}));
		}
		drop(state);
		if !jobs.is_empty() {
			self.render_worker.submit(jobs);
		}
	}

	pub(super) fn rerender_stale(&mut self, channel_id: Snowflake) {
		let stale = self.state.read().stale_rendered(channel_id, self.epoch);
		if stale.is_empty() {
			return;
		}
		self.render_messages(stale);
		let mut changes = Changes::default();
		changes.messages.insert(channel_id);
		self.notifier.push(changes);
	}

	pub(super) fn on_rendered(&mut self, results: Vec<RenderResult>) {
		let mut changes = Changes::default();
		let mut state = self.state.write();
		for RenderResult { message, rendered } in results {
			if rendered.revision < self.epoch {
				continue;
			}
			changes.merge(state.apply(&Event::Local(LocalEvent::MessageRendered {
				channel_id: message.channel_id,
				id: message.id,
				source: message,
				rendered,
			})));
		}
		drop(state);
		self.notifier.push(changes);
	}

	pub(super) fn forward_connection(&self, connected: bool) {
		if self.flags.events {
			let status = if connected { "connected" } else { "disconnected" };
			self.hub
				.manager
				.dispatch_event(&PluginEvent::Connection(status.to_owned()));
		}
	}

	pub(super) fn forward_voice(&mut self) {
		if !self.flags.events {
			return;
		}
		let info = voice_info(&self.state.read());
		if info == self.last_voice_info {
			return;
		}
		if let Some(info) = &info {
			self.hub.manager.dispatch_voice_changed(info.clone());
		}
		self.last_voice_info = info;
	}

	pub(super) fn forward_channel_switch(&self, guild_id: Option<Snowflake>, channel_id: Snowflake) {
		if self.flags.events {
			self.hub
				.manager
				.dispatch_event(&PluginEvent::ChannelSwitched(ChannelSwitch {
					guild_id: guild_id.map(Snowflake::get),
					channel_id: channel_id.get(),
				}));
		}
	}

	pub(super) fn forward_dispatch(&self, event: &DispatchEvent) {
		if !self.flags.events {
			return;
		}
		let events = self.plugin_events(event);
		for event in &events {
			self.hub.manager.dispatch_event(event);
		}
	}

	fn plugin_events(&self, event: &DispatchEvent) -> Vec<PluginEvent> {
		let state = self.state.read();
		let deleted = |id: Snowflake, channel_id: Snowflake, guild_id: Option<Snowflake>| {
			PluginEvent::MessageDeleted(MessageRef {
				id: id.get(),
				channel_id: channel_id.get(),
				guild_id: guild_id
					.or_else(|| state.channel_guild(channel_id))
					.map(Snowflake::get),
			})
		};
		match event {
			DispatchEvent::MessageCreate(message) => {
				vec![PluginEvent::MessageCreated(view_of(message, || {
					state.channel_guild(message.channel_id)
				}))]
			}
			DispatchEvent::MessageUpdate(update) => {
				let guild = || state.channel_guild(update.channel_id);
				let view = match state
					.messages(update.channel_id)
					.and_then(|cache| cache.by_id.get(&update.id))
				{
					Some(message) => Some(view_of(message, guild)),
					None => uncached_update(update).map(|message| view_of(&message, guild)),
				};
				view.map(PluginEvent::MessageUpdated).into_iter().collect()
			}
			DispatchEvent::MessageDelete(delete) => {
				vec![deleted(delete.id, delete.channel_id, delete.guild_id)]
			}
			DispatchEvent::MessageDeleteBulk(delete) => delete
				.ids
				.iter()
				.map(|&id| deleted(id, delete.channel_id, delete.guild_id))
				.collect(),
			DispatchEvent::MessageReactionAdd(add) => vec![PluginEvent::ReactionAdded(ReactionEvent {
				message: MessageRef {
					id: add.message_id.get(),
					channel_id: add.channel_id.get(),
					guild_id: add
						.guild_id
						.or_else(|| state.channel_guild(add.channel_id))
						.map(Snowflake::get),
				},
				user_id: add.user_id.get(),
				emoji: emoji_text(&add.emoji),
			})],
			DispatchEvent::TypingStart(typing) => vec![PluginEvent::Typing(TypingEvent {
				channel_id: typing.channel_id.get(),
				user_id: typing.user_id.get(),
			})],
			_ => Vec::new(),
		}
	}

	pub(super) fn hook_before_send(
		&self,
		nonce: Nonce,
		channel_id: Snowflake,
		content: String,
		reply_to: Option<Snowflake>,
		files: Vec<UploadFile>,
	) {
		let manager = Arc::clone(&self.hub.manager);
		let internal = self.internal.clone();
		let outgoing = OutgoingMessage {
			channel_id: channel_id.get(),
			content: content.clone(),
			reply_to: reply_to.map(Snowflake::get),
		};
		self.runtime.spawn_blocking(move || {
			let decision = manager.before_send(outgoing);
			let _ = internal.send(Internal::BeforeSend {
				nonce,
				channel_id,
				content,
				reply_to,
				files,
				decision,
			});
		});
	}

	pub(super) fn on_before_send(
		&mut self,
		nonce: Nonce,
		channel_id: Snowflake,
		content: String,
		reply_to: Option<Snowflake>,
		files: Vec<UploadFile>,
		decision: SendDecision,
	) {
		match decision {
			SendDecision::Pass => self.dispatch_send(nonce, channel_id, content, reply_to, files),
			SendDecision::Replaced { by, msg } => {
				self.apply_local(LocalEvent::PendingAltered {
					nonce: nonce.clone(),
					content: msg.content.clone(),
					by,
				});
				self.dispatch_send(nonce, channel_id, msg.content, reply_to, files);
			}
			SendDecision::Cancelled { by, reason } => self.apply_local(LocalEvent::PendingFailed {
				nonce,
				reason: Some(format!("{by}: {reason}")),
			}),
		}
	}

	pub(super) fn schedule_overrides(&mut self, delay: Duration) {
		if !self.flags.display {
			return;
		}
		let due = Instant::now() + delay;
		self.overrides_due = Some(self.overrides_due.map_or(due, |current| current.min(due)));
	}

	pub(super) fn watch_overrides(&mut self, changes: &Changes) {
		if !self.flags.display {
			return;
		}
		let selected = self.state.read().selected();
		let visible_messages =
			selected.is_some_and(|selected| changes.messages.contains(selected.channel_id));
		if visible_messages || !changes.members.is_empty() {
			self.schedule_overrides(OVERRIDE_DEBOUNCE);
		}
	}

	pub(super) fn refresh_overrides(&mut self) {
		self.overrides_due = None;
		if !self.flags.display {
			return;
		}
		if self.overrides_busy {
			self.schedule_overrides(OVERRIDE_DEBOUNCE);
			return;
		}
		let targets = self.override_targets();
		if targets.is_empty() {
			return;
		}
		self.overrides_busy = true;
		let manager = Arc::clone(&self.hub.manager);
		let internal = self.internal.clone();
		self.runtime.spawn_blocking(move || {
			let entries: Vec<OverrideEntry> = targets
				.into_iter()
				.map(|(guild_id, user)| {
					let over = manager
						.display_user(guild_id.map(Snowflake::get), &user_view(&user))
						.and_then(to_override);
					(guild_id, user.id, over)
				})
				.collect();
			let _ = internal.send(Internal::Overrides(entries));
		});
	}

	pub(super) fn on_overrides(&mut self, entries: Vec<OverrideEntry>) {
		self.overrides_busy = false;
		let more = entries.len() >= OVERRIDE_BATCH;
		self.apply_local(LocalEvent::UserOverrides { entries });
		if more {
			self.schedule_overrides(Duration::ZERO);
		}
	}

	fn override_targets(&self) -> Vec<(Option<Snowflake>, User)> {
		let state = self.state.read();
		let Some(selected) = state.selected() else {
			return Vec::new();
		};
		let guild_id = selected.guild_id;
		let mut seen = HashSet::new();
		let mut targets = Vec::new();
		let mut consider = |user: &User| {
			if targets.len() < OVERRIDE_BATCH
				&& !state.has_override_entry(guild_id, user.id)
				&& seen.insert(user.id)
			{
				targets.push((guild_id, user.clone()));
			}
		};
		if let Some(cache) = state.messages(selected.channel_id) {
			for message in cache.ordered.iter().rev().take(OVERRIDE_AUTHOR_WINDOW) {
				consider(&message.author);
			}
		}
		if let Some(guild) = guild_id.and_then(|id| state.guild(id)) {
			for slot in &guild.member_list.slots {
				if let MemberSlot::Member(user_id) = slot
					&& let Some(user) = state.user(*user_id)
				{
					consider(user);
				}
			}
		}
		targets
	}

	fn plugin_task(
		&self,
		what: &'static str,
		reload: bool,
		task: impl FnOnce(&PluginHub) -> Result<()> + Send + 'static,
	) {
		let hub = Arc::clone(&self.hub);
		let internal = self.internal.clone();
		self.queue_plugin_job(move || {
			let outcome = task(&hub);
			hub.record(what, outcome);
			let _ = internal.send(if reload {
				Internal::PluginsChanged
			} else {
				Internal::SlotsChanged
			});
		});
	}

	pub(super) fn on_plugin_command(&mut self, command: Command) {
		match command {
			Command::RunPluginCommand {
				name,
				args,
				channel_id,
			} => self.plugin_task("plugin command", true, move |hub| {
				hub.manager.command(&name, &args, channel_id.get())?;
				Ok(())
			}),
			Command::PluginUiAction {
				plugin_id,
				id,
				payload,
			} => {
				self.plugin_task("plugin action", true, move |hub| {
					hub.manager.ui_action(&plugin_id, &id, &payload)?;
					Ok(())
				});
			}
			Command::PluginEnable(id) => self.plugin_task("enable plugin", true, move |hub| {
				hub.manager.enable(&id)?;
				hub.sync_themes();
				Ok(())
			}),
			Command::PluginDisable(id) => self.plugin_task("disable plugin", true, move |hub| {
				hub.manager.disable(&id)?;
				hub.sync_themes();
				Ok(())
			}),
			Command::PluginReload(id) => self.plugin_task("reload plugin", true, move |hub| {
				hub.manager.reload(&id)?;
				hub.sync_themes();
				Ok(())
			}),
			Command::PluginInstall { path } => {
				self.plugin_task("install plugin", true, move |hub| hub.install(&path, None));
			}
			Command::PluginUninstall(id) => {
				self.plugin_task("uninstall plugin", true, move |hub| hub.uninstall(&id));
			}
			Command::PluginGrant {
				id,
				permissions,
				hosts,
			} => self.plugin_task("grant permissions", true, move |hub| {
				hub.grant(&id, &permissions, &hosts)
			}),
			Command::PluginSetSetting { id, key, value } => {
				self.plugin_task("plugin setting", true, move |hub| {
					hub.manager.set_setting(&id, &key, &value)?;
					Ok(())
				});
			}
			Command::PluginsChanged => self.on_plugins_changed(),
			Command::CatalogRefresh { url } => {
				let hub = Arc::clone(&self.hub);
				let internal = self.internal.clone();
				self.runtime.spawn(async move {
					hub.set_catalog(fetch_catalog(&url).await);
					let _ = internal.send(Internal::CatalogChanged);
				});
			}
			Command::CatalogInstall { id } => {
				let hub = Arc::clone(&self.hub);
				let internal = self.internal.clone();
				self.runtime.spawn(async move {
					let outcome = install_from_catalog(Arc::clone(&hub), &id).await;
					hub.record("install from catalog", outcome);
					let _ = internal.send(Internal::PluginsChanged);
				});
			}
			Command::ThemeCatalogRefresh { url } => {
				let hub = Arc::clone(&self.hub);
				let internal = self.internal.clone();
				self.runtime.spawn(async move {
					hub.set_theme_catalog(fetch_theme_catalog(&url).await);
					let _ = internal.send(Internal::CatalogChanged);
				});
			}
			Command::ThemeInstall { id } => {
				let hub = Arc::clone(&self.hub);
				let internal = self.internal.clone();
				self.runtime.spawn(async move {
					match install_theme_from_catalog(Arc::clone(&hub), &id).await {
						Ok(()) => hub.clear_theme_catalog_error(),
						Err(error) => hub.set_theme_catalog_error(error),
					}
					let _ = internal.send(Internal::CatalogChanged);
				});
			}
			_ => {}
		}
	}
}
