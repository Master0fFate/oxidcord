use std::collections::HashSet;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use plugin_host::api::model::{Color, Presence};
use plugin_host::api::{
	ChannelSwitch, DisplayOverride, Event as PluginEvent, MessageRef, MessageView, ReactionEvent, TypingEvent,
};
use plugin_host::convert::user_view;
use plugin_host::{DisplayAnswer, FullSendDecision, PACKAGE_EXTENSION, Permission, SlotEntry};
use protocol::{DispatchEvent, Message, MessageUpdate, Nonce, PartialEmoji, Snowflake, User};
use state::{
	BadgeColor, Changes, Event, LocalEvent, MemberSlot, PluginBadge, RenderedMessage, SharedState,
	UserOverride, apply_update,
};
use tokio::runtime::Handle;
use tokio::sync::{Barrier, mpsc};
use tokio::time::Instant;

use super::outgoing::Outgoing;
use super::{Core, Internal};
use crate::broker::view_of;
use crate::catalog::{download_plugin, download_theme, fetch_catalog, fetch_theme_catalog, write_theme};
use crate::command::Command;
use crate::plugins::{PluginFlags, PluginHub};
use crate::render::{RenderJob, RenderResult};
use crate::views::{message_meta, presence};

const OVERRIDE_DEBOUNCE: Duration = Duration::from_secs(2);
const OVERRIDE_BATCH: usize = 200;
const OVERRIDE_AUTHOR_WINDOW: usize = 100;
const FILTER_RECHECK: usize = 100;
const ACCESSORY_SLOT: &str = "message-accessory";

type OverrideEntry = (Option<Snowflake>, Snowflake, Option<UserOverride>);
type OverrideTarget = (Option<Snowflake>, User, Option<Presence>);

type PluginJob = Box<dyn FnOnce() + Send>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskKind {
	Lifecycle,
	Setting,
}

enum LaneItem {
	Run(PluginJob),
	Fence(Arc<Barrier>),
}

#[derive(Clone, Copy)]
pub(super) enum Lane {
	Interactive,
	Background,
	Gesture,
}

pub(super) struct PluginLanes {
	runtime: Handle,
	interactive: mpsc::UnboundedSender<LaneItem>,
	background: mpsc::UnboundedSender<LaneItem>,
	gesture: mpsc::UnboundedSender<LaneItem>,
}

async fn run_blocking(job: PluginJob) {
	if let Err(error) = tokio::task::spawn_blocking(job).await {
		tracing::warn!(target: "plugin", %error, "plugin job panicked");
	}
}

fn spawn_lane(runtime: &Handle) -> mpsc::UnboundedSender<LaneItem> {
	let (sender, mut items) = mpsc::unbounded_channel::<LaneItem>();
	runtime.spawn(async move {
		while let Some(item) = items.recv().await {
			match item {
				LaneItem::Run(job) => run_blocking(job).await,
				LaneItem::Fence(gate) => {
					gate.wait().await;
					gate.wait().await;
				}
			}
		}
	});
	sender
}

fn queue_item(lane: &mpsc::UnboundedSender<LaneItem>, item: LaneItem) {
	if lane.send(item).is_err() {
		tracing::warn!(target: "plugin", "plugin job queue is closed");
	}
}

impl PluginLanes {
	pub(super) fn spawn(runtime: &Handle) -> Self {
		Self {
			runtime: runtime.clone(),
			interactive: spawn_lane(runtime),
			background: spawn_lane(runtime),
			gesture: spawn_lane(runtime),
		}
	}

	pub(super) fn run(&self, lane: Lane, job: impl FnOnce() + Send + 'static) {
		let lane = match lane {
			Lane::Interactive => &self.interactive,
			Lane::Background => &self.background,
			Lane::Gesture => &self.gesture,
		};
		queue_item(lane, LaneItem::Run(Box::new(job)));
	}

	pub(super) fn fenced(&self, job: impl FnOnce() + Send + 'static) {
		let gate = Arc::new(Barrier::new(4));
		queue_item(&self.interactive, LaneItem::Fence(Arc::clone(&gate)));
		queue_item(&self.background, LaneItem::Fence(Arc::clone(&gate)));
		queue_item(&self.gesture, LaneItem::Fence(Arc::clone(&gate)));
		self.runtime.spawn(async move {
			gate.wait().await;
			run_blocking(Box::new(job)).await;
			gate.wait().await;
		});
	}
}

struct RenderMarks {
	patches: u64,
	accessory: Arc<[SlotEntry]>,
}

impl RenderMarks {
	fn capture(hub: &PluginHub) -> Self {
		Self {
			patches: hub.manager.runtime().patches().revision(),
			accessory: hub.manager.slots().get(ACCESSORY_SLOT),
		}
	}

	fn changed(&self, hub: &PluginHub, state: &SharedState, actor: Option<&str>) -> bool {
		self.patches != hub.manager.runtime().patches().revision()
			|| !Arc::ptr_eq(&self.accessory, &hub.manager.slots().get(ACCESSORY_SLOT))
			|| (can_filter(hub, actor) && filter_changed(hub, state))
	}
}

fn can_filter(hub: &PluginHub, actor: Option<&str>) -> bool {
	actor.is_none_or(|plugin_id| {
		hub.manager
			.runtime()
			.grants()
			.has(plugin_id, Permission::MessageFilter)
	})
}

fn filter_changed(hub: &PluginHub, state: &SharedState) -> bool {
	if !hub.manager.has_active(Permission::MessageFilter) {
		return false;
	}
	let checks: Vec<(Arc<MessageView>, bool)> = {
		let state = state.read();
		let Some(selected) = state.selected() else {
			return false;
		};
		let Some(cache) = state.messages(selected.channel_id) else {
			return false;
		};
		cache
			.ordered
			.iter()
			.rev()
			.take(FILTER_RECHECK)
			.filter(|message| !cache.is_deleted(message.id))
			.filter_map(|message| {
				let rendered = cache.rendered.get(&message.id)?;
				let view = view_of(message, || state.channel_guild(selected.channel_id));
				Some((Arc::new(view), rendered.hidden))
			})
			.collect()
	};
	checks
		.iter()
		.any(|(view, hidden)| hub.manager.filter_message(view) == *hidden)
}

pub(super) fn emoji_text(emoji: &PartialEmoji) -> String {
	match (&emoji.name, emoji.id) {
		(Some(name), Some(id)) => format!("{name}:{id}"),
		(Some(name), None) => name.clone(),
		(None, Some(id)) => id.to_string(),
		(None, None) => String::new(),
	}
}

fn plugin_color(color: Color) -> BadgeColor {
	match color {
		Color::Token(token) => BadgeColor::Token(token),
		Color::Rgba(rgba) => BadgeColor::Rgba(rgba),
	}
}

pub(super) fn plugin_badge(badge: plugin_host::api::model::Badge) -> PluginBadge {
	PluginBadge {
		icon: badge.icon,
		image: badge.image,
		label: badge.label,
		tooltip: badge.tooltip,
		color: badge.color.map(plugin_color),
		key: badge.key,
	}
}

fn legacy_override(over: DisplayOverride) -> Option<UserOverride> {
	let color = over
		.color
		.as_deref()
		.and_then(|color| u32::from_str_radix(color.trim_start_matches('#'), 16).ok());
	(over.name.is_some() || color.is_some() || over.badge.is_some()).then_some(UserOverride {
		name: over.name,
		color,
		badge: over.badge,
		plugin: None,
		suffix: None,
		badges: Vec::new(),
		avatar: None,
		banner: None,
		decoration: None,
		pronouns: None,
	})
}

fn to_override(answer: DisplayAnswer) -> Option<UserOverride> {
	match answer {
		DisplayAnswer::Legacy(over) => legacy_override(over),
		DisplayAnswer::Full(plugin, over) => Some(UserOverride {
			name: over.name,
			color: match over.color {
				Some(Color::Rgba(rgba)) => Some(rgba >> 8),
				_ => None,
			},
			badge: None,
			plugin: Some(plugin.to_string()),
			suffix: over.suffix,
			badges: over.badges.into_iter().map(plugin_badge).collect(),
			avatar: over.avatar,
			banner: over.banner,
			decoration: over.decoration,
			pronouns: over.pronouns,
		}),
	}
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
		self.plugin_lanes.fenced(move || {
			hub.load_all(safe_mode);
			let _ = internal.send(Internal::PluginsChanged);
		});
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
		self.overrides_generation += 1;
		self.apply_local(LocalEvent::UserOverridesCleared);
		self.sync_plugin_hooks();
		let selected = self.state.read().selected();
		if let Some(selected) = selected {
			self.rerender_stale(selected.channel_id);
		}
		self.schedule_overrides(Duration::ZERO);
		self.refresh_plugin_shell();
		self.notifier.push(Changes {
			plugins: true,
			plugin_slots: true,
			..Changes::default()
		});
	}

	pub(super) fn on_surfaces_changed(&mut self, rerender: bool) {
		if rerender {
			self.epoch += 1;
			self.render_worker.set_epoch(self.epoch);
			let selected = self.state.read().selected();
			if let Some(selected) = selected {
				self.rerender_stale(selected.channel_id);
			}
		}
		self.on_slots_changed();
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
		let intercept = self.flags.intercept;
		let with_meta = self.flags.hooks.intercept_ext;
		let mut state = self.state.write();
		let me = state.current_user_id();
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
					mine: me == Some(message.author.id),
					intercept,
					meta: with_meta.then(|| message_meta(&state, state.messages(channel_id), &message)),
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
				.dispatch_event(PluginEvent::Connection(status.to_owned()));
		}
	}

	pub(super) fn forward_voice(&mut self) {
		if !self.flags.events {
			return;
		}
		let info = self.voice_cache.get(&self.state);
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
				.dispatch_event(PluginEvent::ChannelSwitched(ChannelSwitch {
					guild_id: guild_id.map(Snowflake::get),
					channel_id: channel_id.get(),
				}));
		}
	}

	pub(super) fn forward_dispatch(&self, event: &DispatchEvent) {
		if !self.flags.timeline {
			return;
		}
		let private_scope = match event {
			DispatchEvent::TypingStart(typing) => {
				self.state.read().channel_guild(typing.channel_id).is_none()
			}
			_ => false,
		};
		for event in self.plugin_events(event) {
			self.hub.manager.dispatch_event_scoped(event, private_scope);
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
		outgoing: Outgoing,
		skip_hook: Option<String>,
	) {
		let manager = Arc::clone(&self.hub.manager);
		let internal = self.internal.clone();
		let full = outgoing.to_full(channel_id);
		self.runtime.spawn_blocking(move || {
			let decision = manager.before_send(full, skip_hook.as_deref());
			let _ = internal.send(Internal::BeforeSend {
				nonce,
				channel_id,
				outgoing,
				decision,
			});
		});
	}

	pub(super) fn on_before_send(
		&mut self,
		nonce: Nonce,
		channel_id: Snowflake,
		outgoing: Outgoing,
		decision: FullSendDecision,
	) {
		match decision {
			FullSendDecision::Pass => self.dispatch_send(nonce, channel_id, outgoing),
			FullSendDecision::Replaced { by, msg } => {
				let outgoing = {
					let state = self.state.read();
					outgoing.rewritten(msg, |sticker| {
						state
							.find_sticker(sticker)
							.is_some_and(|known| known.available != Some(false))
					})
				};
				self.apply_local(LocalEvent::PendingAltered {
					nonce: nonce.clone(),
					content: outgoing.content.clone(),
					by,
				});
				self.dispatch_send(nonce, channel_id, outgoing);
			}
			FullSendDecision::Cancelled { by, reason } => {
				let reason = format!("{by}: {reason}");
				if outgoing.shows_pending() {
					self.apply_local(LocalEvent::PendingFailed {
						nonce,
						reason: Some(reason),
					});
				} else {
					self.reject(reason);
				}
			}
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
		let epoch = self.overrides_generation;
		self.plugin_lanes.run(Lane::Background, move || {
			let mut users = Vec::with_capacity(targets.len());
			let mut keys = Vec::with_capacity(targets.len());
			for (guild_id, user, presence) in targets {
				users.push((guild_id.map(Snowflake::get), user_view(&user), presence));
				keys.push((guild_id, user.id));
			}
			let entries: Vec<OverrideEntry> = manager
				.display_users(&users)
				.into_iter()
				.zip(keys)
				.map(|(answer, (guild_id, user_id))| (guild_id, user_id, answer.and_then(to_override)))
				.collect();
			let _ = internal.send(Internal::Overrides { epoch, entries });
		});
	}

	pub(super) fn on_overrides(&mut self, epoch: u64, entries: Vec<OverrideEntry>) {
		self.overrides_busy = false;
		if epoch != self.overrides_generation {
			self.schedule_overrides(Duration::ZERO);
			return;
		}
		for (_, user_id, _) in &entries {
			self.invalidated_users.remove(user_id);
		}
		let more = entries.len() >= OVERRIDE_BATCH;
		self.apply_local(LocalEvent::UserOverrides { entries });
		if more {
			self.schedule_overrides(Duration::ZERO);
		}
	}

	fn override_targets(&self) -> Vec<OverrideTarget> {
		let with_presence = self.flags.hooks.presence_events;
		let state = self.state.read();
		let Some(selected) = state.selected() else {
			return Vec::new();
		};
		let guild_id = selected.guild_id;
		let mut seen = HashSet::new();
		let mut targets = Vec::new();
		let mut consider = |user: &User| {
			if targets.len() < OVERRIDE_BATCH
				&& (!state.has_override_entry(guild_id, user.id) || self.invalidated_users.contains(&user.id))
				&& seen.insert(user.id)
			{
				targets.push((
					guild_id,
					user.clone(),
					with_presence.then(|| presence(&state, user.id)).flatten(),
				));
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
		kind: TaskKind,
		what: &'static str,
		task: impl FnOnce(&PluginHub) -> Result<()> + Send + 'static,
	) {
		let hub = Arc::clone(&self.hub);
		let internal = self.internal.clone();
		let job = move || {
			let outcome = task(&hub);
			hub.record(what, outcome);
			let _ = internal.send(Internal::PluginsChanged);
		};
		match kind {
			TaskKind::Lifecycle => self.plugin_lanes.fenced(job),
			TaskKind::Setting => self.plugin_lanes.run(Lane::Interactive, job),
		}
	}

	fn interactive_task(
		&self,
		actor: Option<String>,
		what: &'static str,
		task: impl FnOnce(&PluginHub) -> Result<()> + Send + 'static,
	) {
		let hub = Arc::clone(&self.hub);
		let internal = self.internal.clone();
		let state = self.state.clone();
		self.plugin_lanes.run(Lane::Gesture, move || {
			let marks = RenderMarks::capture(&hub);
			let outcome = task(&hub);
			hub.record(what, outcome);
			let rerender = marks.changed(&hub, &state, actor.as_deref());
			let _ = internal.send(Internal::SurfacesChanged { rerender });
		});
	}

	pub(super) fn on_plugin_command(&mut self, command: Command) {
		match command {
			Command::RunPluginCommand {
				name,
				args,
				channel_id,
			} => self.interactive_task(None, "plugin command", move |hub| {
				hub.manager.command(&name, &args, channel_id.get())?;
				Ok(())
			}),
			Command::PluginUiAction {
				plugin_id,
				id,
				payload,
			} => {
				let actor = Some(plugin_id.clone());
				self.interactive_task(actor, "plugin action", move |hub| {
					hub.manager.ui_action(&plugin_id, &id, &payload)?;
					Ok(())
				});
			}
			Command::PluginViewEvent {
				plugin_id,
				event,
				detail,
			} => {
				let actor = Some(plugin_id.clone());
				self.interactive_task(actor, "plugin view event", move |hub| {
					hub.manager.view_event(&plugin_id, event, detail)?;
					Ok(())
				});
			}
			Command::PluginMenuItem {
				plugin_id,
				menu,
				key,
				context,
				detail,
			} => {
				let actor = Some(plugin_id.clone());
				self.interactive_task(actor, "plugin menu item", move |hub| {
					hub.manager.menu_item(&plugin_id, &menu, &key, context, detail)?;
					Ok(())
				});
			}
			Command::PluginRenderView {
				point,
				context,
				detail,
			} => {
				let hub = Arc::clone(&self.hub);
				let internal = self.internal.clone();
				self.plugin_lanes.run(Lane::Background, move || {
					if hub.manager.render_views(&point, context, detail) {
						let _ = internal.send(Internal::SlotsChanged);
					}
				});
			}
			Command::PluginEmojiPick { emoji, channel_id } => {
				let hub = Arc::clone(&self.hub);
				let internal = self.internal.clone();
				self.plugin_lanes.run(Lane::Interactive, move || {
					hub.manager.pick_emoji(&emoji, channel_id.get());
					let _ = internal.send(Internal::SlotsChanged);
				});
			}
			Command::PluginAutocompleteEmoji {
				query,
				generation,
				candidates,
			} => {
				self.request_autocomplete(query, generation, candidates);
			}
			Command::PluginMessageClick(click) => self.hub.manager.message_click(&click),
			Command::PluginChannelClick(click) => self.hub.manager.channel_click(&click),
			Command::PluginLinkClick { url, context, reply } => {
				let hub = Arc::clone(&self.hub);
				self.plugin_lanes.run(Lane::Interactive, move || {
					reply.send(hub.manager.link_click(&url, &context));
				});
			}
			Command::PluginKeybind { chord } => {
				let bound = self.hub.manager.runtime().keybinds().lookup(&chord);
				if let Some((plugin_id, id)) = bound {
					self.hub.manager.keybind(&plugin_id, &id);
				}
			}
			Command::PluginBadgeClick {
				plugin_id,
				user_id,
				key,
			} => self.hub.manager.badge_click(&plugin_id, user_id.get(), &key),
			Command::PluginEnable(id) => self.plugin_task(TaskKind::Lifecycle, "enable plugin", move |hub| {
				hub.manager.enable(&id)?;
				hub.sync_themes();
				Ok(())
			}),
			Command::PluginDisable(id) => {
				self.plugin_task(TaskKind::Lifecycle, "disable plugin", move |hub| {
					hub.manager.disable(&id)?;
					hub.sync_themes();
					Ok(())
				})
			}
			Command::PluginReload(id) => self.plugin_task(TaskKind::Lifecycle, "reload plugin", move |hub| {
				hub.manager.reload(&id)?;
				hub.sync_themes();
				Ok(())
			}),
			Command::PluginInstall { path } => {
				self.plugin_task(TaskKind::Lifecycle, "install plugin", move |hub| {
					hub.install(&path, None)
				});
			}
			Command::PluginUninstall(id) => {
				self.forget_plugin_shell(&id);
				self.plugin_task(TaskKind::Lifecycle, "uninstall plugin", move |hub| {
					hub.uninstall(&id)
				});
			}
			Command::PluginGrant {
				id,
				permissions,
				hosts,
			} => self.plugin_task(TaskKind::Lifecycle, "grant permissions", move |hub| {
				hub.grant(&id, &permissions, &hosts)
			}),
			Command::PluginSetSetting { id, key, value } => {
				self.plugin_task(TaskKind::Setting, "plugin setting", move |hub| {
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
