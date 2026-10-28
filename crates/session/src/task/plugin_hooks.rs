use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use plugin_host::api::model::{
	ChannelDetail, ClientEvent, EmojiInfo, NotificationInfo, NotificationKind, NotificationVerdict,
};
use plugin_host::api::{MessageRef, ReactionEvent};
use protocol::{ChannelType, DispatchEvent, Presence, Snowflake};
use state::{Alert, AlertVerdict, ChannelDecoration, LocalEvent};
use tokio::time::Instant;

use super::plugin_link::{Lane, plugin_badge};
use super::{Core, Internal};
use crate::views;

const IDLE_POLL: Duration = Duration::from_secs(30);
const IDLE_THRESHOLD_SECS: u64 = 600;
const EVENT_TICK: Duration = Duration::from_millis(100);
const AUTOCOMPLETE_DEBOUNCE: Duration = Duration::from_millis(100);
const DECORATE_DEBOUNCE: Duration = Duration::from_millis(200);
const ALERT_DEADLINE: Duration = Duration::from_millis(500);
const UNREAD_BACKLOG: usize = 256;
const DECORATE_LIMIT: usize = 300;
const DMS_DECORATED: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Probe {
	None,
	Known,
	Unknown,
}

#[derive(Default)]
pub(super) struct HookState {
	plugins_started: bool,
	ready_seen: bool,
	idle: bool,
	idle_due: Option<Instant>,
	unread: VecDeque<Snowflake>,
	unread_queued: HashSet<Snowflake>,
	list_dirty: bool,
	flush_due: Option<Instant>,
	autocomplete: Option<(String, u64, Vec<EmojiInfo>)>,
	autocomplete_due: Option<Instant>,
	decorations_due: Option<Instant>,
	decorating: bool,
	decorated: HashSet<Snowflake>,
	generation: u64,
	notified_alert: u64,
	alert_deadlines: VecDeque<(u64, Instant)>,
}

impl HookState {
	pub(super) fn next_deadline(&self) -> Option<Instant> {
		[
			self.idle_due,
			self.flush_due,
			self.autocomplete_due,
			self.decorations_due,
			self.alert_deadlines.front().map(|(_, due)| *due),
		]
		.into_iter()
		.flatten()
		.min()
	}
}

fn is_thread(kind: ChannelType) -> bool {
	matches!(
		kind,
		ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::AnnouncementThread
	)
}

fn is_private(kind: ChannelType) -> bool {
	matches!(kind, ChannelType::Dm | ChannelType::GroupDm)
}

fn alert_verdict(verdict: NotificationVerdict) -> AlertVerdict {
	match verdict {
		NotificationVerdict::Allow => AlertVerdict::Allow,
		NotificationVerdict::Silent => AlertVerdict::Silent,
		NotificationVerdict::Suppress => AlertVerdict::Suppress,
		NotificationVerdict::Replace(over) => AlertVerdict::Replace {
			title: over.title,
			body: over.body,
			sound: over.sound,
		},
	}
}

impl Core {
	pub(super) fn sync_plugin_hooks(&mut self) {
		let hooks = self.flags.hooks;
		self.apply_local(LocalEvent::AlertHold(hooks.notification));
		if !hooks.notification {
			self.resolve_pending_alerts();
		}
		self.hooks.idle_due = match (hooks.presence_events, self.hooks.idle_due) {
			(true, None) => Some(Instant::now() + IDLE_POLL),
			(true, due) => due,
			(false, _) => None,
		};
		self.hooks.generation += 1;
		self.hooks.decorated.clear();
		self.apply_local(LocalEvent::DecorationsCleared);
		self.schedule_decorations();
		self.hooks.plugins_started = true;
		self.emit_ready();
	}

	fn resolve_pending_alerts(&mut self) {
		let pending = self.state.read().pending_alerts();
		for alert in pending {
			self.apply_local(LocalEvent::AlertResolved {
				id: alert.id,
				verdict: AlertVerdict::Allow,
			});
		}
	}

	pub(super) fn client_event(&self, event: ClientEvent, private_scope: bool) {
		if self.flags.hooks.client_events {
			self.hub.manager.dispatch_client_event(event, private_scope);
		}
	}

	pub(super) fn note_ready(&mut self) {
		self.hooks.ready_seen = true;
		self.emit_ready();
	}

	fn emit_ready(&mut self) {
		if self.hooks.plugins_started && self.hooks.ready_seen && self.flags.hooks.client_events {
			self.hooks.ready_seen = false;
			self.client_event(ClientEvent::Ready, false);
		}
	}

	pub(super) fn probe_dispatch(&self, event: &DispatchEvent) -> Probe {
		if !self.flags.hooks.client_events {
			return Probe::None;
		}
		let guild_id = match event {
			DispatchEvent::GuildCreate(guild) => guild.id,
			DispatchEvent::GuildDelete(guild) => guild.id,
			_ => return Probe::None,
		};
		if self.state.read().guild(guild_id).is_some() {
			Probe::Known
		} else {
			Probe::Unknown
		}
	}

	pub(super) fn forward_client_events(&mut self, event: &DispatchEvent, probe: Probe) {
		if !self.flags.hooks.client_events {
			return;
		}
		match event {
			DispatchEvent::RelationshipAdd(relationship) => {
				let view = views::relationship(&self.state.read(), relationship);
				if let Some(view) = view {
					self.client_event(ClientEvent::RelationshipChanged(view), false);
				}
			}
			DispatchEvent::RelationshipRemove(remove) => {
				self.client_event(ClientEvent::RelationshipRemoved(remove.id.get()), false);
			}
			DispatchEvent::GuildCreate(guild) => {
				if probe == Probe::Unknown {
					self.client_event(ClientEvent::GuildJoined(guild.id.get()), false);
				}
				self.note_list_changed();
			}
			DispatchEvent::GuildDelete(guild) => {
				if probe == Probe::Known && guild.unavailable != Some(true) {
					self.client_event(ClientEvent::GuildLeft(guild.id.get()), false);
				}
				self.note_list_changed();
			}
			DispatchEvent::ChannelDelete(channel) if is_private(channel.kind) => {
				self.client_event(ClientEvent::DmClosed(channel.id.get()), true);
				self.note_list_changed();
			}
			DispatchEvent::ChannelCreate(channel)
			| DispatchEvent::ChannelUpdate(channel)
			| DispatchEvent::ChannelDelete(channel)
				if !is_thread(channel.kind) =>
			{
				self.note_list_changed();
			}
			DispatchEvent::UserSettingsProtoUpdate(_) => self.note_list_changed(),
			DispatchEvent::MessageReactionRemove(remove) => {
				let guild_id = remove
					.guild_id
					.or_else(|| self.state.read().channel_guild(remove.channel_id));
				self.client_event(
					ClientEvent::ReactionRemoved(ReactionEvent {
						message: MessageRef {
							id: remove.message_id.get(),
							channel_id: remove.channel_id.get(),
							guild_id: guild_id.map(Snowflake::get),
						},
						user_id: remove.user_id.get(),
						emoji: super::plugin_link::emoji_text(&remove.emoji),
					}),
					false,
				);
			}
			DispatchEvent::MessageCreate(message) => self.note_acked(message),
			DispatchEvent::PresenceUpdate(presence) => self.note_presence(presence),
			_ => {}
		}
	}

	fn note_acked(&self, message: &protocol::Message) {
		let (latency, guild_id) = {
			let state = self.state.read();
			(
				state
					.messages(message.channel_id)
					.and_then(|cache| cache.latency_ms(message.id)),
				message
					.guild_id
					.or_else(|| state.channel_guild(message.channel_id)),
			)
		};
		if let Some(latency) = latency {
			self.client_event(
				ClientEvent::MessageAcked((
					MessageRef {
						id: message.id.get(),
						channel_id: message.channel_id.get(),
						guild_id: guild_id.map(Snowflake::get),
					},
					latency,
				)),
				false,
			);
		}
	}

	fn note_presence(&mut self, presence: &Presence) {
		let Some(user_id) = presence
			.user_id
			.or_else(|| presence.user.as_ref().map(|user| user.id))
		else {
			return;
		};
		let (view, tracked) = {
			let state = self.state.read();
			let mine = state.current_user_id() == Some(user_id);
			let friend = state
				.relationship(user_id)
				.is_some_and(|relationship| relationship.kind == protocol::RelationshipKind::Friend);
			let tracked = state
				.selected()
				.is_some_and(|selected| state.has_override_entry(selected.guild_id, user_id));
			(
				(mine || friend).then(|| views::presence_view(user_id, presence)),
				tracked,
			)
		};
		if let Some(view) = view {
			self.client_event(ClientEvent::PresenceChanged(view), false);
		}
		if tracked && self.flags.display {
			self.invalidated_users.insert(user_id);
			self.schedule_overrides(Duration::from_secs(2));
		}
	}

	pub(super) fn note_unread(&mut self, channel_id: Snowflake) {
		if !self.flags.hooks.read_state_events {
			return;
		}
		if self.hooks.unread_queued.len() < UNREAD_BACKLOG && self.hooks.unread_queued.insert(channel_id) {
			self.hooks.unread.push_back(channel_id);
		}
		self.arm_flush();
	}

	pub(super) fn note_list_changed(&mut self) {
		self.hooks.decorated.clear();
		self.schedule_decorations();
		if self.flags.hooks.client_events {
			self.hooks.list_dirty = true;
			self.arm_flush();
		}
	}

	fn arm_flush(&mut self) {
		if self.hooks.flush_due.is_none() {
			self.hooks.flush_due = Some(Instant::now() + EVENT_TICK);
		}
	}

	fn flush_client_events(&mut self) {
		self.hooks.flush_due = None;
		if std::mem::take(&mut self.hooks.list_dirty) {
			self.client_event(ClientEvent::ChannelListChanged, false);
		}
		if let Some(channel_id) = self.hooks.unread.pop_front() {
			self.hooks.unread_queued.remove(&channel_id);
			let found = {
				let state = self.state.read();
				state.channel_unread(channel_id).map(|unread| {
					(
						views::read_state(unread),
						state.channel_guild(channel_id).is_none(),
					)
				})
			};
			if let Some((read_state, private)) = found {
				self.client_event(ClientEvent::UnreadChanged(read_state), private);
			}
		}
		if !self.hooks.unread.is_empty() {
			self.arm_flush();
		}
	}

	fn poll_idle(&mut self) {
		self.hooks.idle_due = None;
		if !self.flags.hooks.presence_events {
			return;
		}
		self.hooks.idle_due = Some(Instant::now() + IDLE_POLL);
		let Some(seconds) = self.sink.idle_seconds() else {
			return;
		};
		let idle = seconds >= IDLE_THRESHOLD_SECS;
		if idle != self.hooks.idle {
			self.hooks.idle = idle;
			self.client_event(ClientEvent::IdleChanged(idle), false);
		}
	}

	pub(super) fn forward_draft(&self, channel_id: Snowflake, content: &str) {
		if !self.flags.hooks.draft {
			return;
		}
		let guild_id = self.state.read().channel_guild(channel_id);
		self.hub
			.manager
			.draft_change(channel_id.get(), guild_id.map(Snowflake::get), content);
	}

	pub(super) fn request_autocomplete(
		&mut self,
		query: String,
		generation: u64,
		candidates: Vec<EmojiInfo>,
	) {
		if !self.flags.hooks.autocomplete {
			return;
		}
		self.hooks.autocomplete = Some((query, generation, candidates));
		self.hooks.autocomplete_due = Some(Instant::now() + AUTOCOMPLETE_DEBOUNCE);
	}

	fn run_autocomplete(&mut self) {
		self.hooks.autocomplete_due = None;
		let Some((query, generation, candidates)) = self.hooks.autocomplete.take() else {
			return;
		};
		let manager = Arc::clone(&self.hub.manager);
		let internal = self.internal.clone();
		self.plugin_lanes.run(Lane::Interactive, move || {
			let order = manager.autocomplete_emoji(&query, &candidates);
			let _ = internal.send(Internal::AutocompleteOrder { generation, order });
		});
	}

	pub(super) fn schedule_decorations(&mut self) {
		if self.flags.hooks.decorate && self.hooks.decorations_due.is_none() {
			self.hooks.decorations_due = Some(Instant::now() + DECORATE_DEBOUNCE);
		}
	}

	pub(super) fn decoration_stale(&mut self, channel_id: Snowflake) {
		self.hooks.decorated.remove(&channel_id);
		self.schedule_decorations();
	}

	fn decoration_targets(&self) -> Vec<ChannelDetail> {
		let state = self.state.read();
		let mut targets = Vec::new();
		let mut consider = |channel_id: Snowflake, private: bool| {
			if targets.len() < DECORATE_LIMIT
				&& !self.hooks.decorated.contains(&channel_id)
				&& let Some(detail) = views::channel_detail(&state, channel_id, private)
			{
				targets.push(detail);
			}
		};
		if let Some(guild) = self.selected_guild.and_then(|guild_id| state.guild(guild_id)) {
			for channel in guild.channels.values() {
				consider(channel.id, false);
			}
		}
		for channel in state.private_channels().take(DMS_DECORATED) {
			consider(channel.id, true);
		}
		targets
	}

	fn run_decorations(&mut self) {
		self.hooks.decorations_due = None;
		if !self.flags.hooks.decorate {
			return;
		}
		if self.hooks.decorating {
			self.hooks.decorations_due = Some(Instant::now() + DECORATE_DEBOUNCE);
			return;
		}
		let targets = self.decoration_targets();
		if targets.is_empty() {
			return;
		}
		self.hooks
			.decorated
			.extend(targets.iter().map(|detail| Snowflake(detail.id)));
		self.hooks.decorating = true;
		let generation = self.hooks.generation;
		let manager = Arc::clone(&self.hub.manager);
		let internal = self.internal.clone();
		self.plugin_lanes.run(Lane::Background, move || {
			let channels: Arc<[ChannelDetail]> = targets.into();
			let mut by_channel: HashMap<u64, Vec<ChannelDecoration>> = HashMap::new();
			for answer in manager.decorate_channels(&channels) {
				for (detail, badges) in channels.iter().zip(answer.badges) {
					if !badges.is_empty() {
						by_channel.entry(detail.id).or_default().push(ChannelDecoration {
							plugin_id: answer.plugin_id.to_string(),
							badges: badges.into_iter().map(plugin_badge).collect(),
						});
					}
				}
			}
			let entries = channels
				.iter()
				.map(|detail| {
					(
						Snowflake(detail.id),
						by_channel.remove(&detail.id).unwrap_or_default(),
					)
				})
				.collect();
			let _ = internal.send(Internal::Decorated { generation, entries });
		});
	}

	pub(super) fn on_decorated(
		&mut self,
		generation: u64,
		entries: Vec<(Snowflake, Vec<ChannelDecoration>)>,
	) {
		self.hooks.decorating = false;
		if generation != self.hooks.generation {
			self.schedule_decorations();
			return;
		}
		self.apply_local(LocalEvent::ChannelDecorations { entries });
	}

	pub(super) fn on_autocomplete_order(&mut self, generation: u64, order: Vec<u32>) {
		self.apply_local(LocalEvent::AutocompleteOrder { generation, order });
	}

	pub(super) fn dispatch_pending_alerts(&mut self) {
		if !self.flags.hooks.notification {
			return;
		}
		let fresh: Vec<Alert> = self
			.state
			.read()
			.pending_alerts()
			.into_iter()
			.filter(|alert| alert.id > self.hooks.notified_alert)
			.collect();
		for alert in fresh {
			self.hooks.notified_alert = self.hooks.notified_alert.max(alert.id);
			let info = self.notification_info(&alert);
			self.hooks
				.alert_deadlines
				.push_back((info.id, Instant::now() + ALERT_DEADLINE));
			let manager = Arc::clone(&self.hub.manager);
			let internal = self.internal.clone();
			self.plugin_lanes.run(Lane::Interactive, move || {
				let verdict = alert_verdict(manager.notification(&info));
				let _ = internal.send(Internal::AlertVerdict { id: info.id, verdict });
			});
		}
	}

	fn notification_info(&self, alert: &Alert) -> NotificationInfo {
		let message = &alert.message;
		let kind = match self
			.state
			.read()
			.channel(message.channel_id)
			.map(|channel| channel.kind)
		{
			Some(ChannelType::Dm) => NotificationKind::Dm,
			Some(ChannelType::GroupDm) => NotificationKind::GroupDm,
			_ => NotificationKind::Message,
		};
		NotificationInfo {
			id: alert.id,
			channel_id: message.channel_id.get(),
			guild_id: alert.guild_id.map(Snowflake::get),
			message: Some(MessageRef {
				id: message.id.get(),
				channel_id: message.channel_id.get(),
				guild_id: alert.guild_id.map(Snowflake::get),
			}),
			author: Some(message.author.id.get()),
			kind,
			mentions_me: alert.mentions_me,
			mentions_everyone: message.mention_everyone,
		}
	}

	pub(super) fn on_alert_verdict(&mut self, id: u64, verdict: AlertVerdict) {
		self.hooks.alert_deadlines.retain(|(pending, _)| *pending != id);
		self.apply_local(LocalEvent::AlertResolved { id, verdict });
	}

	pub(super) fn on_hook_timer(&mut self) {
		let now = Instant::now();
		let due = |at: Option<Instant>| at.is_some_and(|at| at <= now);
		while let Some((id, _)) = self.hooks.alert_deadlines.pop_front_if(|(_, at)| *at <= now) {
			self.apply_local(LocalEvent::AlertResolved {
				id,
				verdict: AlertVerdict::Allow,
			});
		}
		if due(self.hooks.flush_due) {
			self.flush_client_events();
		}
		if due(self.hooks.idle_due) {
			self.poll_idle();
		}
		if due(self.hooks.autocomplete_due) {
			self.run_autocomplete();
		}
		if due(self.hooks.decorations_due) {
			self.run_decorations();
		}
	}
}
