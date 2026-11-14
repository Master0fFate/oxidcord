use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use indexmap::IndexMap;
use parking_lot::RwLock;
use protocol::{
	Channel, Message, MessageFlags, Nonce, Presence, ProtoStatus, ReadState, Relationship, Snowflake,
	StatusKind, User, UserGuildSettings,
};
use smallvec::SmallVec;

use crate::changes::Changes;
use crate::event::{Event, LoadDirection, LocalEvent};
use crate::guild::{Application, GuildState};
use crate::interactions::{ModalPrompt, PendingInteraction};
use crate::loaded::Lazy;
use crate::messages::ChannelMessages;
use crate::notifications::Alert;
use crate::plugin_shell::{AutocompleteOrder, PluginShell};
use crate::settings::GuildFolder;
use crate::views::RenderedMessage;
use crate::views::UserOverride;
use crate::voice::{Call, MicTest, StreamSummary, VoiceCueEntry, VoiceLevels, VoiceSession, VoiceStats};

pub const TYPING_TIMEOUT: Duration = Duration::from_secs(10);
const EVENT_NOTICE_BACKLOG: usize = 16;

pub type SharedState = Arc<RwLock<State>>;

#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
	pub session_id: String,
	pub auth_session_id_hash: Option<String>,
	pub resume_url: Option<String>,
	pub connected: bool,
	pub signed_out: bool,
	pub latency: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
	pub guild_id: Option<Snowflake>,
	pub channel_id: Snowflake,
}

#[derive(Debug, Default)]
pub struct State {
	pub(crate) current_user: Option<User>,
	pub(crate) guilds: IndexMap<Snowflake, GuildState>,
	pub(crate) applications: IndexMap<Snowflake, Application>,
	pub(crate) private_channels: IndexMap<Snowflake, Channel>,
	pub(crate) channel_guilds: HashMap<Snowflake, Option<Snowflake>>,
	pub(crate) users: HashMap<Snowflake, Arc<User>>,
	pub(crate) presences: HashMap<Snowflake, Presence>,
	pub(crate) relationships: IndexMap<Snowflake, Relationship>,
	pub(crate) read_states: HashMap<Snowflake, ReadState>,
	pub(crate) guild_settings: HashMap<Snowflake, UserGuildSettings>,
	pub(crate) global_settings: Option<UserGuildSettings>,
	pub(crate) messages: HashMap<Snowflake, ChannelMessages>,
	pub(crate) typing: HashMap<Snowflake, HashMap<Snowflake, Instant>>,
	pub(crate) slowmode: HashMap<Snowflake, Instant>,
	pub(crate) thread_cooldown: HashMap<Snowflake, Instant>,
	pub(crate) session: SessionInfo,
	pub(crate) guild_folders: Vec<GuildFolder>,
	pub(crate) guild_positions: Vec<Snowflake>,
	pub(crate) proto_status: Vec<u8>,
	pub(crate) proto_privacy: Vec<u8>,
	pub(crate) status_settings: ProtoStatus,
	pub(crate) privacy: protocol::ProtoPrivacy,
	pub(crate) proto_text_and_images: Vec<u8>,
	pub(crate) sensitive_media: protocol::SensitiveMedia,
	pub(crate) collapsed_categories: HashSet<Snowflake>,
	pub(crate) revealed_blocked: HashSet<Snowflake>,
	pub(crate) drafts: HashMap<Snowflake, String>,
	pub(crate) selected: Option<Selection>,
	pub(crate) voice: Option<VoiceSession>,
	pub(crate) mic_test: Option<MicTest>,
	pub(crate) account_switch: Option<Snowflake>,
	pub(crate) calls: HashMap<Snowflake, Call>,
	pub(crate) altered: HashMap<Snowflake, String>,
	pub(crate) sent_by_plugin: HashMap<Snowflake, String>,
	pub(crate) user_overrides: HashMap<(Option<Snowflake>, Snowflake), Option<UserOverride>>,
	stale_user_overrides: HashSet<(Option<Snowflake>, Snowflake)>,
	pub(crate) unrendered: Vec<Arc<Message>>,
	pub(crate) keep_deleted: bool,
	pub(crate) lazy: Lazy,
	pub(crate) streams: HashMap<String, StreamSummary>,
	pub(crate) rejection: Option<Rejection>,
	pub(crate) rejection_count: u64,
	pub(crate) interactions: HashMap<Nonce, PendingInteraction>,
	pub(crate) modal: Option<ModalPrompt>,
	pub(crate) modal_count: u64,
	pub(crate) friend_request: Option<SentRequest>,
	pub(crate) friend_request_count: u64,
	pub(crate) friend_notice: Option<FriendNotice>,
	pub(crate) friend_notice_count: u64,
	pub(crate) event_notices: VecDeque<EventNotice>,
	pub(crate) event_notice_count: u64,
	pub(crate) chosen_status: Option<StatusKind>,
	pub(crate) afk: bool,
	pub(crate) alerts: VecDeque<Alert>,
	pub(crate) alert_count: u64,
	pub(crate) voice_cues: VecDeque<VoiceCueEntry>,
	pub(crate) voice_cue_count: u64,
	pub(crate) plugin_shell: PluginShell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
	pub id: u64,
	pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentRequest {
	pub id: u64,
	pub username: String,
	pub outcome: RequestOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FriendNotice {
	pub id: u64,
	pub user_id: Snowflake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventNotice {
	pub id: u64,
	pub guild_id: Snowflake,
	pub event_id: Snowflake,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome {
	Sent,
	Failed(String),
}

fn deadline_left(deadlines: &HashMap<Snowflake, Instant>, channel_id: Snowflake) -> Option<Duration> {
	deadlines
		.get(&channel_id)
		.map(|until| until.saturating_duration_since(Instant::now()))
		.filter(|remaining| !remaining.is_zero())
}

fn time_left(
	deadlines: &HashMap<Snowflake, Instant>,
	channel_id: Snowflake,
	interval: Duration,
) -> Option<Duration> {
	deadline_left(deadlines, channel_id).map(|remaining| remaining.min(interval))
}

fn arm(deadlines: &mut HashMap<Snowflake, Instant>, channel_id: Snowflake, wait: Duration) -> Changes {
	let now = Instant::now();
	deadlines.retain(|_, until| *until > now);
	deadlines.insert(channel_id, now + wait);
	let mut changes = Changes::default();
	changes.slowmode.insert(channel_id);
	changes
}

pub(crate) fn intern_user(users: &mut HashMap<Snowflake, Arc<User>>, user: &User) {
	if user.username.is_empty() {
		return;
	}
	if users.get(&user.id).is_some_and(|existing| **existing == *user) {
		return;
	}
	users.insert(user.id, Arc::new(user.clone()));
}

impl State {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn shared() -> SharedState {
		Arc::new(RwLock::new(Self::new()))
	}

	pub fn apply(&mut self, event: &Event<'_>) -> Changes {
		match event {
			Event::Ready(ready) => self.apply_ready(ready),
			Event::ReadySupplemental(supplemental) => self.apply_supplemental(supplemental),
			Event::Dispatch(dispatch) => self.apply_dispatch(dispatch),
			Event::MessagesLoaded {
				channel_id,
				messages,
				direction,
				reached_start,
			} => self.apply_messages_loaded(*channel_id, messages, *direction, *reached_start),
			Event::Local(local) => self.apply_local(local),
		}
	}

	pub fn expire(&mut self, now: Instant) -> Changes {
		let mut changes = Changes::default();
		self.typing.retain(|&channel_id, users| {
			let before = users.len();
			users.retain(|_, started| now.saturating_duration_since(*started) < TYPING_TIMEOUT);
			if users.len() != before {
				changes.typing.insert(channel_id);
			}
			!users.is_empty()
		});
		changes
	}

	pub fn current_user(&self) -> Option<&User> {
		self.current_user.as_ref()
	}

	pub fn current_user_id(&self) -> Option<Snowflake> {
		self.current_user.as_ref().map(|user| user.id)
	}

	pub fn guild(&self, id: Snowflake) -> Option<&GuildState> {
		self.guilds.get(&id)
	}

	pub fn guilds(&self) -> impl Iterator<Item = &GuildState> {
		self.guilds.values()
	}

	pub fn channel(&self, id: Snowflake) -> Option<&Channel> {
		match self.channel_guilds.get(&id)? {
			Some(guild_id) => self.guilds.get(guild_id)?.channel(id),
			None => self.private_channels.get(&id),
		}
	}

	pub fn channel_guild(&self, channel_id: Snowflake) -> Option<Snowflake> {
		self.channel_guilds.get(&channel_id).copied().flatten()
	}

	pub fn is_private(&self, channel_id: Snowflake) -> bool {
		matches!(self.channel_guilds.get(&channel_id), Some(None))
	}

	pub fn private_channels(&self) -> impl Iterator<Item = &Channel> {
		self.private_channels.values()
	}

	pub fn user(&self, id: Snowflake) -> Option<&Arc<User>> {
		self.users.get(&id)
	}

	pub fn presence(&self, user_id: Snowflake) -> Option<&Presence> {
		self.presences.get(&user_id)
	}

	pub fn relationships(&self) -> impl Iterator<Item = &Relationship> {
		self.relationships.values()
	}

	pub fn relationship(&self, user_id: Snowflake) -> Option<&Relationship> {
		self.relationships.get(&user_id)
	}

	pub fn read_state(&self, channel_id: Snowflake) -> Option<&ReadState> {
		self.read_states.get(&channel_id)
	}

	pub fn guild_settings(&self, guild_id: Option<Snowflake>) -> Option<&UserGuildSettings> {
		match guild_id {
			Some(id) => self.guild_settings.get(&id),
			None => self.global_settings.as_ref(),
		}
	}

	pub fn hides_muted_channels(&self, guild_id: Snowflake) -> bool {
		self.guild_settings
			.get(&guild_id)
			.is_some_and(|settings| settings.hide_muted_channels)
	}

	pub fn messages(&self, channel_id: Snowflake) -> Option<&ChannelMessages> {
		self.messages.get(&channel_id)
	}

	pub fn altered_by(&self, message_id: Snowflake) -> Option<&str> {
		self.altered.get(&message_id).map(String::as_str)
	}

	pub fn sent_by_plugin(&self, message_id: Snowflake) -> Option<&str> {
		self.sent_by_plugin.get(&message_id).map(String::as_str)
	}

	pub fn user_override(&self, guild_id: Option<Snowflake>, user_id: Snowflake) -> Option<&UserOverride> {
		self.user_overrides.get(&(guild_id, user_id))?.as_ref()
	}

	pub fn has_override_entry(&self, guild_id: Option<Snowflake>, user_id: Snowflake) -> bool {
		let key = (guild_id, user_id);
		self.user_overrides.contains_key(&key) && !self.stale_user_overrides.contains(&key)
	}

	pub fn take_unrendered(&mut self) -> Vec<Arc<Message>> {
		std::mem::take(&mut self.unrendered)
	}

	pub fn stale_rendered(&self, channel_id: Snowflake, revision: u64) -> Vec<Arc<Message>> {
		let Some(cache) = self.messages.get(&channel_id) else {
			return Vec::new();
		};
		cache
			.ordered
			.iter()
			.filter(|message| {
				cache
					.rendered
					.get(&message.id)
					.is_none_or(|rendered| rendered.revision < revision)
			})
			.cloned()
			.collect()
	}

	pub(crate) fn note_rejection(&mut self, reason: String) {
		self.rejection_count += 1;
		self.rejection = Some(Rejection {
			id: self.rejection_count,
			reason,
		});
	}

	pub fn last_rejection(&self) -> Option<&Rejection> {
		self.rejection.as_ref()
	}

	fn note_friend_request(&mut self, username: String, outcome: RequestOutcome) {
		self.friend_request_count += 1;
		self.friend_request = Some(SentRequest {
			id: self.friend_request_count,
			username,
			outcome,
		});
	}

	pub fn last_friend_request(&self) -> Option<&SentRequest> {
		self.friend_request.as_ref()
	}

	pub(crate) fn note_friend_notice(&mut self, user_id: Snowflake) {
		self.friend_notice_count += 1;
		self.friend_notice = Some(FriendNotice {
			id: self.friend_notice_count,
			user_id,
		});
	}

	pub fn last_friend_notice(&self) -> Option<FriendNotice> {
		self.friend_notice
	}

	pub(crate) fn note_event_notice(&mut self, guild_id: Snowflake, event_id: Snowflake) {
		self.event_notice_count += 1;
		if self.event_notices.len() >= EVENT_NOTICE_BACKLOG {
			self.event_notices.pop_front();
		}
		self.event_notices.push_back(EventNotice {
			id: self.event_notice_count,
			guild_id,
			event_id,
		});
	}

	pub fn last_event_notice(&self) -> Option<EventNotice> {
		self.event_notices.back().copied()
	}

	pub fn event_notices_since(&self, seen: u64) -> Vec<EventNotice> {
		self.event_notices
			.iter()
			.filter(|notice| notice.id > seen)
			.copied()
			.collect()
	}

	pub fn selected(&self) -> Option<Selection> {
		self.selected
	}

	pub fn account_switch_requested(&self) -> Option<Snowflake> {
		self.account_switch
	}

	pub fn session(&self) -> &SessionInfo {
		&self.session
	}

	pub fn draft(&self, channel_id: Snowflake) -> Option<&str> {
		self.drafts.get(&channel_id).map(String::as_str)
	}

	pub fn guild_folders(&self) -> &[GuildFolder] {
		&self.guild_folders
	}

	pub fn is_collapsed(&self, category_id: Snowflake) -> bool {
		self.collapsed_categories.contains(&category_id)
	}

	pub fn typing_users(&self, channel_id: Snowflake, now: Instant) -> SmallVec<[Snowflake; 4]> {
		let me = self.current_user_id();
		self.typing
			.get(&channel_id)
			.into_iter()
			.flatten()
			.filter(|(user_id, started)| {
				Some(**user_id) != me && now.saturating_duration_since(**started) < TYPING_TIMEOUT
			})
			.map(|(user_id, _)| *user_id)
			.collect()
	}

	pub fn slowmode_remaining(&self, channel_id: Snowflake) -> Option<Duration> {
		time_left(&self.slowmode, channel_id, self.slowmode_interval(channel_id)?)
	}

	pub fn thread_cooldown_remaining(&self, parent_id: Snowflake) -> Option<Duration> {
		deadline_left(&self.thread_cooldown, parent_id)
	}

	pub(crate) fn start_slowmode(&mut self, channel_id: Snowflake, wait: Duration) -> Changes {
		arm(&mut self.slowmode, channel_id, wait)
	}

	pub(crate) fn start_thread_cooldown(&mut self, parent_id: Snowflake, wait: Duration) -> Changes {
		arm(&mut self.thread_cooldown, parent_id, wait)
	}

	pub(crate) fn drop_messages(&mut self, channel_id: Snowflake) -> bool {
		let Some(cache) = self.messages.remove(&channel_id) else {
			return false;
		};
		if !self.revealed_blocked.is_empty() {
			for message in &cache.ordered {
				self.revealed_blocked.remove(&message.id);
			}
		}
		true
	}

	pub(crate) fn channel_mut(&mut self, id: Snowflake) -> Option<&mut Channel> {
		match self.channel_guilds.get(&id)? {
			Some(guild_id) => self.guilds.get_mut(guild_id)?.channel_mut(id),
			None => self.private_channels.get_mut(&id),
		}
	}

	pub(crate) fn mark_channel_dirty(&self, channel_id: Snowflake, changes: &mut Changes) {
		match self.channel_guilds.get(&channel_id) {
			Some(Some(guild_id)) => {
				changes.channel_lists.insert(*guild_id);
				changes.guild_list = true;
			}
			Some(None) => changes.private_channels = true,
			None => {}
		}
	}

	fn apply_messages_loaded(
		&mut self,
		channel_id: Snowflake,
		messages: &[Message],
		direction: LoadDirection,
		reached_start: bool,
	) -> Changes {
		let guild_id = self.channel_guild(channel_id);
		let mut loaded = Vec::with_capacity(messages.len());
		for message in messages {
			self.ingest_message_people(guild_id, message);
			loaded.push(Arc::new(message.clone()));
		}
		self.unrendered.extend(loaded.iter().cloned());
		let channel_last = self
			.channel(channel_id)
			.and_then(|channel| channel.last_message_id);
		self.messages.entry(channel_id).or_default().merge_loaded(
			loaded,
			direction,
			reached_start,
			channel_last,
		);
		let mut changes = Changes::default();
		changes.messages.insert(channel_id);
		changes
	}

	fn apply_local(&mut self, event: &LocalEvent) -> Changes {
		let mut changes = Changes::default();
		match event {
			LocalEvent::PendingMessage {
				channel_id,
				nonce,
				content,
				files,
				author,
				by_plugin,
				silent,
			} => {
				let rendered = Arc::new(RenderedMessage::plain(content, false, 0));
				let message = Message {
					channel_id: *channel_id,
					guild_id: self.channel_guild(*channel_id),
					author: (**author).clone(),
					content: content.clone(),
					timestamp: Some(Utc::now()),
					nonce: Some(nonce.clone()),
					flags: if *silent {
						MessageFlags::SUPPRESS_NOTIFICATIONS
					} else {
						MessageFlags::default()
					},
					..Message::default()
				};
				self.messages.entry(*channel_id).or_default().add_pending(
					Arc::new(message),
					files.as_slice().into(),
					by_plugin.clone(),
					rendered,
				);
				changes.messages.insert(*channel_id);
			}
			LocalEvent::PendingFailed {
				nonce,
				reason,
				retryable,
			} => {
				for (&channel_id, cache) in &mut self.messages {
					if cache.fail_pending(nonce, reason.clone(), *retryable) {
						changes.messages.insert(channel_id);
					}
				}
			}
			LocalEvent::PendingRetried { nonce } => {
				for (&channel_id, cache) in &mut self.messages {
					if cache.retry_pending(nonce) {
						changes.messages.insert(channel_id);
					}
				}
			}
			LocalEvent::UploadProgress { nonce, uploaded } => {
				for (&channel_id, cache) in &mut self.messages {
					if cache.upload_progress(nonce, *uploaded) {
						changes.messages.insert(channel_id);
					}
				}
			}
			LocalEvent::PendingAltered { nonce, content, by } => {
				for (&channel_id, cache) in &mut self.messages {
					if cache.alter_pending(nonce, content.clone(), by.clone()) {
						changes.messages.insert(channel_id);
					}
				}
			}
			LocalEvent::MessageRendered {
				channel_id,
				id,
				source,
				rendered,
			} => {
				if let Some(cache) = self.messages.get_mut(channel_id)
					&& cache
						.by_id
						.get(id)
						.is_some_and(|current| current.content == source.content)
					&& cache.is_deleted(*id) == rendered.deleted
				{
					cache.rendered.insert(*id, Arc::clone(rendered));
					changes.messages.insert(*channel_id);
				}
			}
			LocalEvent::KeepDeleted(keep) => {
				self.keep_deleted = *keep;
				if !keep {
					for (&channel_id, cache) in &mut self.messages {
						if cache.drop_deleted() {
							changes.messages.insert(channel_id);
						}
					}
				}
			}
			LocalEvent::UserOverrides { entries } => {
				for (guild_id, user_id, over, complete) in entries {
					let key = (*guild_id, *user_id);
					if *complete {
						self.stale_user_overrides.remove(&key);
					} else {
						self.stale_user_overrides.insert(key);
					}
					if self.user_overrides.get(&key) != Some(over) {
						self.user_overrides.insert(key, over.clone());
						changes.messages.mark_all();
						changes.members.mark_all();
					}
				}
			}
			LocalEvent::UserOverridesInvalidated { users } => {
				self.stale_user_overrides.extend(
					self.user_overrides
						.keys()
						.filter(|(_, user_id)| users.contains(*user_id))
						.copied(),
				);
			}
			LocalEvent::UserOverridesCleared => {
				self.stale_user_overrides.clear();
				if !self.user_overrides.is_empty() {
					self.user_overrides.clear();
					changes.messages.mark_all();
					changes.members.mark_all();
				}
			}
			LocalEvent::PendingDiscarded { nonce } => {
				for (&channel_id, cache) in &mut self.messages {
					if cache.remove_pending(nonce) {
						changes.messages.insert(channel_id);
					}
				}
			}
			LocalEvent::SlowmodeStarted { channel_id, wait } => {
				return self.start_slowmode(*channel_id, *wait);
			}
			LocalEvent::ThreadCooldownStarted { parent_id, wait } => {
				return self.start_thread_cooldown(*parent_id, *wait);
			}
			LocalEvent::DraftChanged { channel_id, content } => {
				if content.is_empty() {
					self.drafts.remove(channel_id);
				} else {
					self.drafts.insert(*channel_id, content.clone());
				}
				changes.drafts.insert(*channel_id);
			}
			LocalEvent::ChannelSelected { guild_id, channel_id } => {
				let selection = Selection {
					guild_id: *guild_id,
					channel_id: *channel_id,
				};
				if self.selected != Some(selection)
					&& let Some(guild) = guild_id.and_then(|id| self.guilds.get_mut(&id))
				{
					guild.member_list.expect_switch();
				}
				self.selected = Some(selection);
				changes.selection = true;
			}
			LocalEvent::SelectionCleared => {
				self.selected = None;
				changes.selection = true;
			}
			LocalEvent::CategoryCollapsed {
				channel_id,
				collapsed,
			} => {
				let changed = if *collapsed {
					self.collapsed_categories.insert(*channel_id)
				} else {
					self.collapsed_categories.remove(channel_id)
				};
				if changed {
					self.mark_channel_dirty(*channel_id, &mut changes);
				}
			}
			LocalEvent::BlockedRunToggled { channel_id, first } => {
				let members: Vec<Snowflake> = self
					.messages
					.get(channel_id)
					.map(|cache| {
						self.blocked_run(cache, *first)
							.map(|message| message.id)
							.collect()
					})
					.unwrap_or_default();
				if members.iter().any(|id| self.revealed_blocked.contains(id)) {
					for id in &members {
						self.revealed_blocked.remove(id);
					}
				} else {
					self.revealed_blocked.insert(*first);
				}
				changes.messages.insert(*channel_id);
			}
			LocalEvent::ConnectionChanged { connected } => {
				self.session.connected = *connected;
				changes.session = true;
			}
			LocalEvent::SignedOut => {
				self.session.connected = false;
				self.session.signed_out = true;
				changes.session = true;
			}
			LocalEvent::LatencyMeasured(latency) => {
				self.session.latency = Some(*latency);
				changes.session = true;
			}
			LocalEvent::VoiceConnection {
				status,
				channel_id,
				guild_id,
			} => return self.voice_connection(status, *channel_id, *guild_id),
			LocalEvent::VoiceSelf { self_mute, self_deaf } => return self.voice_self(*self_mute, *self_deaf),
			LocalEvent::Speaking { user_id, speaking } => return self.voice_speaking(*user_id, *speaking),
			LocalEvent::VoiceStats {
				ping_ms,
				loss,
				jitter,
				e2ee,
				video_bitrate_bps,
			} => {
				return self.voice_stats(VoiceStats {
					ping_ms: *ping_ms,
					loss: *loss,
					jitter_ms: *jitter,
					e2ee: *e2ee,
					video_bitrate_bps: *video_bitrate_bps,
				});
			}
			LocalEvent::MicTestStarted { seconds } => {
				return self.mic_test_changed(Some(MicTest {
					level: 0.0,
					remaining_secs: *seconds,
				}));
			}
			LocalEvent::MicTestLevel {
				level,
				remaining_secs,
			} => {
				return self.mic_test_changed(Some(MicTest {
					level: *level,
					remaining_secs: *remaining_secs,
				}));
			}
			LocalEvent::MicTestFinished => return self.mic_test_changed(None),
			LocalEvent::AccountSwitchRequested(user_id) => {
				self.account_switch = Some(*user_id);
				changes.session = true;
			}
			LocalEvent::VoiceLevels { input, output } => {
				return self.voice_levels(VoiceLevels {
					input: *input,
					output: *output,
				});
			}
			LocalEvent::HandRequested(requested) => return self.hand_requested_changed(*requested),
			LocalEvent::VoiceE2ee(info) => return self.voice_e2ee(info.clone()),
			LocalEvent::VoiceE2eeFailed => return self.voice_e2ee_failed(),
			LocalEvent::SettingsEdited => changes.user_settings = true,
			LocalEvent::GuildLayout(layout) => return self.apply_guild_layout(layout),
			LocalEvent::RestoreGuildLayout { before, applied } => {
				if self.guild_layout() != *applied {
					return Changes::default();
				}
				return self.apply_guild_layout(before);
			}
			LocalEvent::RestorePrivacy { before, applied } => {
				if self.proto_privacy != *applied {
					return Changes::default();
				}
				return self.replace_privacy_settings(before.clone(), false);
			}
			LocalEvent::RestoreSensitiveMedia { before, applied } => {
				if self.proto_text_and_images != *applied {
					return Changes::default();
				}
				return self.replace_sensitive_media(before, false);
			}
			LocalEvent::CameraChanged(on) => return self.camera_changed(*on),
			LocalEvent::ScreenShareChanged(info) => return self.screen_share_changed(info.clone()),
			LocalEvent::Watching(stream_key) => return self.watching_changed(stream_key.clone()),
			LocalEvent::StreamPreview { stream_key, url } => {
				return self.stream_preview_changed(stream_key, url);
			}
			LocalEvent::RemoteVideo {
				user_id,
				kind,
				active,
			} => {
				return self.remote_video_changed(*user_id, *kind, *active);
			}
			LocalEvent::Loaded(loaded) => return self.apply_loaded(loaded),
			LocalEvent::CommandRejected { reason } => {
				self.note_rejection(reason.clone());
				changes.rejected = true;
			}
			LocalEvent::InteractionStarted {
				nonce,
				channel_id,
				message_id,
			} => return self.interaction_started(nonce, *channel_id, *message_id),
			LocalEvent::InteractionEnded { nonce } => return self.interaction_ended(nonce),
			LocalEvent::EphemeralDismissed {
				channel_id,
				message_id,
			} => return self.ephemeral_dismissed(*channel_id, *message_id),
			LocalEvent::FriendRequestSent { username } => {
				self.note_friend_request(username.clone(), RequestOutcome::Sent);
				changes.friend_request = true;
			}
			LocalEvent::FriendRequestFailed { username, reason } => {
				self.note_friend_request(username.clone(), RequestOutcome::Failed(reason.clone()));
				changes.friend_request = true;
			}
			LocalEvent::StatusChosen(status) => {
				self.chosen_status = Some(status.clone());
				changes.own_status = true;
			}
			LocalEvent::CustomStatusExpired => return self.expire_custom_status(),
			LocalEvent::AfkChanged(afk) => {
				changes.own_status = self.afk != *afk;
				self.afk = *afk;
			}
			LocalEvent::UiIntent(intent) => {
				self.plugin_shell.push_intent(intent.clone());
				changes.intents = true;
			}
			LocalEvent::ClientOptionsChanged(options) => {
				if self.plugin_shell.client_options != *options {
					self.plugin_shell.client_options = *options;
					changes.client_options = true;
				}
			}
			LocalEvent::AlertHold(hold) => self.plugin_shell.hold_alerts = *hold,
			LocalEvent::AlertResolved { id, verdict } => {
				changes.alerts = self.resolve_alert(*id, verdict);
			}
			LocalEvent::AutocompleteOrder { generation, order } => {
				self.plugin_shell.autocomplete = Some(AutocompleteOrder {
					generation: *generation,
					order: order.clone(),
				});
				changes.autocomplete = true;
			}
			LocalEvent::ChannelDecorations { entries } => {
				for (channel_id, decorations) in entries {
					let current = self.plugin_shell.decorations.get(channel_id);
					if current.map(Vec::as_slice).unwrap_or_default() == decorations.as_slice() {
						continue;
					}
					if decorations.is_empty() {
						self.plugin_shell.decorations.remove(channel_id);
					} else {
						self.plugin_shell
							.decorations
							.insert(*channel_id, decorations.clone());
					}
					self.mark_channel_dirty(*channel_id, &mut changes);
				}
			}
			LocalEvent::DecorationsCleared => {
				if !self.plugin_shell.decorations.is_empty() {
					self.plugin_shell.decorations.clear();
					changes.guild_list = true;
					changes.private_channels = true;
					changes.channel_lists.mark_all();
				}
			}
			LocalEvent::DmSectionsChanged(sections) => {
				if self.plugin_shell.dm_sections != *sections {
					self.plugin_shell.dm_sections.clone_from(sections);
					changes.dm_sections = true;
					changes.private_channels = true;
				}
			}
		}
		changes
	}
}
