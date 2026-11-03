use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant as StdInstant};

use activity::ActivityHandle;
use chrono::Utc;
use gateway::{AUTHENTICATION_FAILED, GatewayError, GatewayEvent, GatewayHandle};
use parking_lot::Mutex;
use plugin_host::api::VoiceInfo;
use plugin_host::api::model::DmSection;
use plugin_host::{FullSendDecision, Invalidation};
use protocol::gateway::IdentifyProperties;
use protocol::{
	AttachmentDescriptor, Channel, ChannelType, DISCORD_EPOCH_MS, DispatchEvent, EditMessage, Message,
	MessageAck, MessageFlags, MessagesQuery, Nonce, ProtoCustomStatus, Snowflake, Token, User, VoiceState,
};
use rest::api::SearchQuery;
use rest::{RestClient, RestError};
use serde_json::{Value, json};
use state::{
	AlertVerdict, Changes, ChannelDecoration, ClientOptions, Event, LoadDirection, Loaded, LocalEvent,
	Selection, SharedState, TYPING_TIMEOUT, UiIntent, UserOverride,
};
use store::{Database, Store};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use crate::broker::{BrokerSink, VoiceCache};
use crate::command::{Command, MEMBER_LIST_WINDOW};
use crate::config::VoiceIo;
use crate::loads::PluginLoads;
use crate::marks::ClientMarks;
use crate::notifier::Notifier;
use crate::persist::{self, Job, Persistor};
use crate::plugins::{PluginFlags, PluginHub};
use crate::render::{RenderResult, RenderWorker};
use crate::search::SearchResults;

mod accounts;
mod actions;
mod commands;
mod expressions;
mod mic_test;
mod outgoing;
mod plugin_actions;
mod plugin_hooks;
mod plugin_link;
mod plugin_load;
mod sending;
mod streams;
mod video;
mod voice_link;

use actions::rest_failure;
use mic_test::MicTestRun;
use outgoing::Outgoing;
pub(crate) use plugin_load::PluginAnswer;
use sending::Sending;
use streams::StreamLink;
pub(crate) use video::SharedVideo;
pub(crate) use voice_link::SharedVoice;
use voice_link::VoiceLink;

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedSelection {
	guild_id: Option<Snowflake>,
	channel_id: Snowflake,
}

const PAGE_SIZE: u8 = 50;
const TYPING_THROTTLE: Duration = Duration::from_secs(9);
const TYPING_EXPIRY_SLACK: Duration = Duration::from_millis(100);
const MEMBER_SEARCH_LIMIT: u32 = 100;
const SEARCH_INDEX_RETRIES: u32 = 2;
const SEARCH_INDEX_MAX_WAIT: Duration = Duration::from_secs(10);
const MEMBER_LIST_WINDOWS_KEPT: u32 = 2;
const DAY_MILLIS: i64 = 86_400_000;
const NONCE_SEQUENCE_BITS: u32 = 22;
const INITIAL_EPOCH: u64 = 1;

#[derive(Default)]
pub(crate) struct Stats {
	pub(crate) reconnects: AtomicU64,
}

#[derive(Clone, Copy)]
pub(crate) struct ReadSnapshot {
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub mention_count: Option<i64>,
}

pub(crate) type SharedSearch = Arc<Mutex<Option<Arc<SearchResults>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Page {
	Older,
	Newer,
	Latest,
}

pub(crate) enum Internal {
	Messages {
		channel_id: Snowflake,
		direction: LoadDirection,
		messages: Vec<Message>,
		reached_start: bool,
		cache: bool,
	},
	LoadFinished(Snowflake, Page),
	Sent {
		nonce: Nonce,
		message: Box<Message>,
	},
	SendFailed {
		nonce: Nonce,
		reason: String,
	},
	SlowmodeHit {
		channel_id: Snowflake,
		retry_after: Option<Duration>,
	},
	UploadProgress {
		nonce: Nonce,
		uploaded: u64,
	},
	Search(SearchResults),
	DmOpened(Box<Channel>),
	ThreadOpened(Box<Channel>),
	SelectJoinedGuild(Snowflake),
	Loaded(Loaded),
	PluginApplied {
		loaded: Loaded,
		answer: PluginAnswer,
	},
	Rejected(String),
	CacheOpened {
		user_id: Snowflake,
		db: Option<Arc<Database>>,
	},
	RestoreRead(Vec<ReadSnapshot>),
	RestoreChannels(Vec<(Snowflake, Option<i64>, Option<Snowflake>)>),
	FriendRequestSent(String),
	FriendRequestFailed {
		username: String,
		reason: String,
	},
	SignedOut,
	CameraStarted {
		generation: u64,
		result: Result<(), String>,
	},
	StreamTimeout {
		generation: u64,
	},
	StreamConnected {
		generation: u64,
		result: Result<Arc<::voice::VoiceConnection>, ::voice::VoiceError>,
	},
	StreamVideo {
		generation: u64,
		result: Result<(), String>,
	},
	StreamEvent {
		generation: u64,
		event: ::voice::VoiceEvent,
	},
	Draft {
		channel_id: Snowflake,
		content: String,
	},
	JoinVoice {
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
	},
	VoiceJoinTimeout(u64),
	VoiceSettled(u64),
	VoiceConnected {
		generation: u64,
		result: Result<Arc<::voice::VoiceConnection>, ::voice::VoiceError>,
	},
	VoiceEvent {
		generation: u64,
		event: ::voice::VoiceEvent,
	},
	VoiceLevels {
		generation: u64,
		input: f32,
		output: f32,
	},
	Rendered(Vec<RenderResult>),
	PluginsChanged,
	SurfacesChanged {
		rerender: bool,
	},
	SlotsChanged,
	CatalogChanged,
	BeforeSend {
		nonce: Nonce,
		channel_id: Snowflake,
		outgoing: Outgoing,
		decision: FullSendDecision,
	},
	AlertVerdict {
		id: u64,
		verdict: AlertVerdict,
	},
	AutocompleteOrder {
		generation: u64,
		order: Vec<u32>,
	},
	Decorated {
		generation: u64,
		entries: Vec<(Snowflake, Vec<ChannelDecoration>)>,
	},
	Overrides {
		epoch: u64,
		entries: Vec<(Option<Snowflake>, Snowflake, Option<UserOverride>)>,
	},
	MicTestLevel {
		generation: u64,
		level: f32,
		remaining_secs: u32,
	},
	MicTestFinished {
		generation: u64,
		result: Result<(), String>,
	},
	AccountSwitch(Snowflake),
	Invalidate {
		plugin: String,
		target: Invalidation,
	},
	Command(Box<Command>),
	Intent(UiIntent),
	Activities(Vec<Value>),
}

pub(crate) struct CoreParts {
	pub(crate) state: SharedState,
	pub(crate) rest: RestClient,
	pub(crate) gateway: GatewayHandle,
	pub(crate) store: Arc<Store>,
	pub(crate) notifier: Notifier,
	pub(crate) runtime: Handle,
	pub(crate) search: SharedSearch,
	pub(crate) stats: Arc<Stats>,
	pub(crate) voice_cache: Arc<VoiceCache>,
	pub(crate) voice_io: VoiceIo,
	pub(crate) shared_voice: SharedVoice,
	pub(crate) video: SharedVideo,
	pub(crate) internal: mpsc::UnboundedSender<Internal>,
	pub(crate) hub: Arc<PluginHub>,
	pub(crate) safe_mode: bool,
	pub(crate) locale: String,
	pub(crate) token: Arc<Token>,
	pub(crate) loads: Arc<PluginLoads>,
	pub(crate) marks: Arc<ClientMarks>,
	pub(crate) media: Option<media::MediaFetcher>,
	pub(crate) save_media: Option<media::MediaFetcher>,
	pub(crate) sink: Arc<dyn BrokerSink>,
	pub(crate) activity: ActivityHandle,
	pub(crate) identity: IdentifyProperties,
}

pub(crate) struct Core {
	state: SharedState,
	rest: RestClient,
	gateway: GatewayHandle,
	store: Arc<Store>,
	persist: Persistor,
	notifier: Notifier,
	runtime: Handle,
	search: SharedSearch,
	stats: Arc<Stats>,
	internal: mpsc::UnboundedSender<Internal>,
	typing_sent: HashMap<Snowflake, StdInstant>,
	loading: HashSet<(Snowflake, Page)>,
	selected_guild: Option<Snowflake>,
	member_channel: Option<Snowflake>,
	member_ranges: Vec<[u32; 2]>,
	command_scopes: HashSet<Snowflake>,
	typing_expiry: Option<Instant>,
	nonce_sequence: u64,
	voice: VoiceLink,
	video: SharedVideo,
	streams: StreamLink,
	custom_status: Option<ProtoCustomStatus>,
	custom_status_due: Option<Instant>,
	hub: Arc<PluginHub>,
	render_worker: RenderWorker,
	plugin_lanes: plugin_link::PluginLanes,
	voice_cache: Arc<VoiceCache>,
	safe_mode: bool,
	locale: String,
	flags: PluginFlags,
	epoch: u64,
	overrides_due: Option<Instant>,
	overrides_busy: bool,
	overrides_generation: u64,
	mic_test: MicTestRun,
	token: Arc<Token>,
	account_saved: bool,
	select_created_guild: bool,
	last_voice_info: Option<VoiceInfo>,
	loads: Arc<PluginLoads>,
	marks: Arc<ClientMarks>,
	media: Option<media::MediaFetcher>,
	save_media: Option<media::MediaFetcher>,
	sink: Arc<dyn BrokerSink>,
	plugin_options: BTreeMap<String, ClientOptions>,
	plugin_dm_sections: BTreeMap<String, Vec<DmSection>>,
	invalidated_users: HashSet<Snowflake>,
	hooks: plugin_hooks::HookState,
	sending: HashMap<Nonce, Sending>,
	activity: ActivityHandle,
	activities: Vec<Value>,
	identity: IdentifyProperties,
}

fn page_query(
	before: Option<Snowflake>,
	after: Option<Snowflake>,
	around: Option<Snowflake>,
) -> MessagesQuery {
	MessagesQuery {
		around,
		before,
		after,
		limit: Some(PAGE_SIZE),
	}
}

fn member_window(index: u32) -> [u32; 2] {
	[index * MEMBER_LIST_WINDOW, (index + 1) * MEMBER_LIST_WINDOW - 1]
}

fn member_ranges(visible: Range<u32>) -> Vec<[u32; 2]> {
	let first = visible.start / MEMBER_LIST_WINDOW;
	let last = visible.end.saturating_sub(1).max(visible.start) / MEMBER_LIST_WINDOW;
	let mut ranges = vec![member_window(0)];
	ranges.extend((first.max(1)..=last.min(first + MEMBER_LIST_WINDOWS_KEPT - 1)).map(member_window));
	ranges
}

fn ack_day() -> i64 {
	(Utc::now().timestamp_millis() - DISCORD_EPOCH_MS) / DAY_MILLIS
}

impl Core {
	pub(crate) fn new(parts: CoreParts) -> Self {
		let persist = Persistor::spawn(&parts.runtime);
		let plugin_lanes = plugin_link::PluginLanes::spawn(&parts.runtime);
		let render_worker = RenderWorker::spawn(
			Arc::clone(&parts.hub.manager),
			Arc::clone(&parts.hub.stats),
			parts.internal.clone(),
			INITIAL_EPOCH,
		);
		Self {
			state: parts.state,
			rest: parts.rest,
			gateway: parts.gateway,
			store: parts.store,
			persist,
			notifier: parts.notifier,
			runtime: parts.runtime,
			search: parts.search,
			stats: parts.stats,
			internal: parts.internal,
			typing_sent: HashMap::new(),
			loading: HashSet::new(),
			selected_guild: None,
			member_channel: None,
			member_ranges: member_ranges(0..0),
			command_scopes: HashSet::new(),
			typing_expiry: None,
			nonce_sequence: 0,
			voice: VoiceLink::new(parts.voice_io, parts.shared_voice),
			video: parts.video,
			streams: StreamLink::default(),
			custom_status: None,
			custom_status_due: None,
			hub: parts.hub,
			render_worker,
			plugin_lanes,
			voice_cache: parts.voice_cache,
			safe_mode: parts.safe_mode,
			locale: parts.locale,
			flags: PluginFlags::default(),
			epoch: INITIAL_EPOCH,
			overrides_due: None,
			overrides_busy: false,
			overrides_generation: 0,
			mic_test: MicTestRun::default(),
			token: parts.token,
			account_saved: false,
			select_created_guild: false,
			last_voice_info: None,
			loads: parts.loads,
			marks: parts.marks,
			media: parts.media,
			save_media: parts.save_media,
			sink: parts.sink,
			plugin_options: BTreeMap::new(),
			plugin_dm_sections: BTreeMap::new(),
			invalidated_users: HashSet::new(),
			hooks: plugin_hooks::HookState::default(),
			sending: HashMap::new(),
			activity: parts.activity,
			activities: Vec::new(),
			identity: parts.identity,
		}
	}

	pub(crate) async fn run(
		mut self,
		mut gateway_events: mpsc::Receiver<GatewayEvent>,
		mut commands: mpsc::Receiver<Command>,
		mut internal: mpsc::UnboundedReceiver<Internal>,
	) {
		self.start_plugins();
		self.restore().await;
		self.restore_recents();
		let mut gateway_open = true;
		loop {
			let flush_at = self.notifier.deadline();
			let expire_at = self.typing_expiry;
			let overrides_at = self.overrides_due;
			let custom_status_at = self.custom_status_due;
			let hooks_at = self.hooks.next_deadline();
			tokio::select! {
				event = gateway_events.recv(), if gateway_open => match event {
					Some(event) => self.on_gateway(event),
					None => gateway_open = false,
				},
				command = commands.recv() => match command {
					Some(command) => {
						if !self.on_command(command) {
							break;
						}
					}
					None => break,
				},
				Some(message) = internal.recv() => self.on_internal(message),
				() = sleep_until(flush_at.unwrap_or_else(Instant::now)), if flush_at.is_some() => {
					self.notifier.flush();
				}
				() = sleep_until(expire_at.unwrap_or_else(Instant::now)), if expire_at.is_some() => {
					self.expire_typing();
				}
				() = sleep_until(custom_status_at.unwrap_or_else(Instant::now)), if custom_status_at.is_some() => {
					self.expire_custom_status();
				}
				() = sleep_until(overrides_at.unwrap_or_else(Instant::now)), if overrides_at.is_some() => {
					self.refresh_overrides();
				}
				() = sleep_until(hooks_at.unwrap_or_else(Instant::now)), if hooks_at.is_some() => {
					self.on_hook_timer();
				}
			}
		}
		self.notifier.flush();
		self.gateway.shutdown();
		self.persist.close().await;
	}

	async fn restore(&mut self) {
		let Some(db) = self.open_current_cache().await else {
			return;
		};
		let cached =
			tokio::task::spawn_blocking(move || (persist::load_ready(&db), persist::load_read_states(&db)))
				.await;
		let Ok((Some(mut ready), read_states)) = cached else {
			return;
		};
		ready.session_id.clear();
		ready.resume_gateway_url = None;
		self.apply(Event::Ready(&ready));
		self.activity.set_user(Some(ready.user.clone()));
		for read_state in read_states {
			let Some(message_id) = read_state.last_message_id else {
				continue;
			};
			self.apply_dispatch(&DispatchEvent::MessageAck(MessageAck {
				channel_id: Snowflake(read_state.channel_id),
				message_id: Snowflake(message_id),
				version: None,
				mention_count: Some(i64::from(read_state.mention_count)),
			}));
		}
		self.restore_selection();
		self.apply_local(LocalEvent::ConnectionChanged { connected: false });
	}

	pub(super) fn restore_selection(&mut self) {
		let Some(saved) = self
			.persist
			.database()
			.and_then(|db| persist::load_value::<SavedSelection>(&db, persist::LAST_SELECTION_KEY))
		else {
			return;
		};
		let reachable = {
			let state = self.state.read();
			state.channel(saved.channel_id).is_some()
				&& state.channel_guild(saved.channel_id) == saved.guild_id
				&& (saved.guild_id.is_none() || state.can_view(saved.channel_id))
		};
		if reachable {
			self.selected_guild = saved.guild_id;
			self.select_channel(saved.guild_id, saved.channel_id);
		}
	}

	fn remember_selection(&self, guild_id: Option<Snowflake>, channel_id: Snowflake) {
		match serde_json::to_vec(&SavedSelection { guild_id, channel_id }) {
			Ok(json) => self.persist.submit(Job::Recent {
				key: persist::LAST_SELECTION_KEY,
				json,
			}),
			Err(error) => tracing::warn!(%error, "could not encode the last selection"),
		}
	}

	fn apply(&mut self, event: Event<'_>) {
		let (changes, fresh, connection) = {
			let mut state = self.state.write();
			let was_connected = state.session().connected;
			let changes = state.apply(&event);
			let connected = state.session().connected;
			(
				changes,
				state.take_unrendered(),
				(connected != was_connected).then_some(connected),
			)
		};
		if let Some(connected) = connection {
			self.forward_connection(connected);
		}
		if !fresh.is_empty() {
			self.render_messages(fresh);
		}
		self.watch_overrides(&changes);
		if changes.voice_session || !changes.voice.is_empty() {
			self.voice_cache.invalidate();
		}
		if changes.voice_session {
			self.forward_voice();
		}
		if !changes.typing.is_empty() {
			self.typing_expiry = Some(Instant::now() + TYPING_TIMEOUT + TYPING_EXPIRY_SLACK);
		}
		if changes.own_status {
			self.schedule_custom_status_expiry();
			self.publish_presence();
		}
		if changes.user_settings {
			self.push_activity_rules();
		}
		if changes.alerts {
			self.dispatch_pending_alerts();
		}
		self.notifier.push(changes);
	}

	fn apply_local(&mut self, event: LocalEvent) {
		self.apply(Event::Local(event));
	}

	fn apply_dispatch(&mut self, event: &DispatchEvent) {
		self.apply(Event::Dispatch(event));
		match event {
			DispatchEvent::MessageCreate(message) => self.note_unread(message.channel_id),
			DispatchEvent::MessageAck(ack) => self.note_unread(ack.channel_id),
			_ => {}
		}
	}

	fn schedule_custom_status_expiry(&mut self) {
		let expires = self.state.read().custom_status_expiry_ms();
		let now_ms = u64::try_from(Utc::now().timestamp_millis()).unwrap_or_default();
		self.custom_status_due =
			expires.map(|expires| Instant::now() + Duration::from_millis(expires.saturating_sub(now_ms)));
	}

	fn expire_custom_status(&mut self) {
		self.custom_status_due = None;
		self.apply_local(LocalEvent::CustomStatusExpired);
		self.schedule_custom_status_expiry();
	}

	fn expire_typing(&mut self) {
		self.typing_expiry = None;
		let changes = self.state.write().expire(StdInstant::now());
		self.notifier.push(changes);
	}

	fn on_gateway(&mut self, event: GatewayEvent) {
		match event {
			GatewayEvent::Connecting { .. } => {
				self.apply_local(LocalEvent::ConnectionChanged { connected: false });
			}
			GatewayEvent::Hello { .. } => self.marks.record("gateway-connected"),
			GatewayEvent::TokenRefreshed(token) => self.refresh_token(token),
			GatewayEvent::Ready(ready) => {
				self.marks.record("ready");
				self.attach_cache(ready.user.id);
				self.apply(Event::Ready(&ready));
				self.activity.set_user(Some(ready.user.clone()));
				self.activity.refresh_connections();
				self.note_ready();
				let stale = {
					let state = self.state.read();
					state
						.selected()
						.is_some_and(|selected| state.channel(selected.channel_id).is_none())
				};
				if stale {
					self.apply_local(LocalEvent::SelectionCleared);
				}
				if self.state.read().selected().is_none() {
					self.restore_selection();
				}
				self.remember_account();
				self.subscribe_selected();
				self.resend_voice_state();
				self.catch_up();
				self.persist.submit(Job::Ready(ready));
			}
			GatewayEvent::ReadySupplemental(supplemental) => {
				self.apply(Event::ReadySupplemental(&supplemental));
				if !supplemental.lazy_private_channels.is_empty() {
					self.persist.submit(Job::PrivateChannels(
						supplemental.lazy_private_channels.into_vec(),
					));
				}
			}
			GatewayEvent::Resumed => {
				self.apply_dispatch(&DispatchEvent::Resumed);
				self.publish_presence();
				self.subscribe_selected();
				self.resend_voice_state();
				self.catch_up();
			}
			GatewayEvent::Dispatch { event, .. } => self.on_dispatch(event),
			GatewayEvent::Disconnected {
				code,
				reason,
				will_resume,
			} => {
				tracing::info!(?code, reason, will_resume, "gateway disconnected");
				self.stats.reconnects.fetch_add(1, Ordering::Relaxed);
				self.apply_local(LocalEvent::ConnectionChanged { connected: false });
			}
			GatewayEvent::Fatal(GatewayError::Fatal {
				code: AUTHENTICATION_FAILED,
			}) => self.on_signed_out(),
			GatewayEvent::Fatal(error) => {
				tracing::error!(%error, "gateway stopped");
				self.apply_local(LocalEvent::ConnectionChanged { connected: false });
			}
			GatewayEvent::Latency { heartbeat_rtt } => {
				self.apply_local(LocalEvent::LatencyMeasured(heartbeat_rtt));
			}
			GatewayEvent::DecodeFailure { event } => {
				tracing::warn!(event, "gateway event could not be decoded");
			}
		}
	}

	fn on_dispatch(&mut self, event: DispatchEvent) {
		let probe = self.probe_dispatch(&event);
		let dropped = self.dropped_channels(&event);
		let selected = matches!(
			event,
			DispatchEvent::ChannelDelete(_) | DispatchEvent::ThreadDelete(_) | DispatchEvent::GuildDelete(_)
		)
		.then(|| self.state.read().selected())
		.flatten();
		let deleted_parent = match &event {
			DispatchEvent::ThreadDelete(delete) => delete.parent_id,
			_ => None,
		};
		self.apply_dispatch(&event);
		if !dropped.is_empty() {
			self.persist.submit(Job::DropChannels(dropped));
			self.prune_sending();
		}
		if let Some(selected) = selected {
			self.reselect_after_delete(selected, deleted_parent);
		}
		self.forward_dispatch(&event);
		self.forward_client_events(&event, probe);
		match event {
			DispatchEvent::MessageCreate(message) => {
				if let Some(nonce) = &message.nonce {
					self.sending.remove(nonce);
				}
				self.persist_cached_message(message.channel_id, message.id);
				self.persist_read_state(message.channel_id);
			}
			DispatchEvent::MessageUpdate(update) => self.persist_cached_message(update.channel_id, update.id),
			DispatchEvent::MessageReactionAdd(reaction) => {
				self.persist_cached_message(reaction.channel_id, reaction.message_id);
			}
			DispatchEvent::MessageReactionAddMany(many) => {
				self.persist_cached_message(many.channel_id, many.message_id);
			}
			DispatchEvent::MessageReactionRemove(reaction) => {
				self.persist_cached_message(reaction.channel_id, reaction.message_id);
			}
			DispatchEvent::MessageReactionRemoveAll(reaction) => {
				self.persist_cached_message(reaction.channel_id, reaction.message_id);
			}
			DispatchEvent::MessageReactionRemoveEmoji(reaction) => {
				self.persist_cached_message(reaction.channel_id, reaction.message_id);
			}
			DispatchEvent::MessagePollVoteAdd(vote) | DispatchEvent::MessagePollVoteRemove(vote) => {
				self.persist_cached_message(vote.channel_id, vote.message_id);
			}
			DispatchEvent::MessageDelete(delete) => {
				self.persist.submit(Job::DeleteMessages(vec![delete.id]));
			}
			DispatchEvent::MessageDeleteBulk(delete) => {
				self.persist.submit(Job::DeleteMessages(delete.ids));
			}
			DispatchEvent::MessageAck(ack) => self.persist_read_state(ack.channel_id),
			DispatchEvent::VoiceStateUpdate(update) => self.voice_state_update(&update),
			DispatchEvent::PassiveUpdateV2(update) if self.voice.target().is_some() => {
				for voice in update.updated_voice_states.iter() {
					self.voice_state_update(&VoiceState {
						guild_id: voice.guild_id.or(Some(update.guild_id)),
						..voice.clone()
					});
				}
			}
			DispatchEvent::VoiceServerUpdate(update) => self.voice_server_update(update),
			DispatchEvent::ChannelUpdate(channel) => self.voice_channel_updated(channel.id),
			DispatchEvent::StreamCreate(stream) => self.stream_created(&stream),
			DispatchEvent::StreamServerUpdate(update) => self.stream_server_updated(update),
			DispatchEvent::StreamDelete(delete) => self.stream_deleted(&delete),
			DispatchEvent::ChannelPinsUpdate(update) => self.reload_pins(update.channel_id),
			DispatchEvent::GuildCreate(guild) => self.select_new_guild(guild.id),
			DispatchEvent::UserUpdate(user) => self.activity.set_user(Some(*user)),
			DispatchEvent::UserConnectionsUpdate(_) => self.activity.refresh_connections(),
			_ => {}
		}
	}

	fn reselect_after_delete(&mut self, previous: Selection, parent: Option<Snowflake>) {
		let fallback = {
			let state = self.state.read();
			if state.selected().is_some() {
				return;
			}
			parent
				.filter(|parent| state.channel(*parent).is_some() && state.can_view(*parent))
				.or_else(|| {
					previous
						.guild_id
						.and_then(|guild_id| state.default_channel(guild_id))
				})
		};
		match fallback {
			Some(channel_id) => self.select_channel(previous.guild_id, channel_id),
			None => self.selected_guild = None,
		}
	}

	fn dropped_channels(&self, event: &DispatchEvent) -> Vec<Snowflake> {
		match event {
			DispatchEvent::ChannelDelete(channel) => vec![channel.id],
			DispatchEvent::ThreadDelete(delete) => vec![delete.id],
			DispatchEvent::GuildDelete(guild) if guild.unavailable != Some(true) => self
				.state
				.read()
				.guild(guild.id)
				.map(|guild| {
					guild
						.channels
						.keys()
						.chain(guild.threads.keys())
						.copied()
						.collect()
				})
				.unwrap_or_default(),
			_ => Vec::new(),
		}
	}

	fn persist_cached_message(&self, channel_id: Snowflake, message_id: Snowflake) {
		let message = self
			.state
			.read()
			.messages(channel_id)
			.and_then(|cache| cache.by_id.get(&message_id).cloned());
		if let Some(message) = message {
			self.persist.submit(Job::Messages {
				channel_id,
				messages: vec![message],
			});
		}
	}

	fn persist_read_state(&self, channel_id: Snowflake) {
		let stored = self
			.state
			.read()
			.read_state(channel_id)
			.map(persist::stored_read_state);
		if let Some(stored) = stored {
			self.persist.submit(Job::ReadState(stored));
		}
	}

	fn on_internal(&mut self, message: Internal) {
		match message {
			Internal::Messages {
				channel_id,
				direction,
				messages,
				reached_start,
				cache,
			} => {
				self.on_messages(channel_id, direction, messages, reached_start, cache);
				self.prune_sending();
			}
			Internal::LoadFinished(channel_id, page) => {
				self.loading.remove(&(channel_id, page));
			}
			Internal::Sent { nonce, message } => self.on_sent(&nonce, message),
			Internal::SendFailed { nonce, reason } => self.on_send_failed(nonce, reason),
			Internal::SlowmodeHit {
				channel_id,
				retry_after,
			} => {
				let wait = retry_after.or_else(|| self.state.read().slowmode_interval(channel_id));
				if let Some(wait) = wait {
					self.apply_local(LocalEvent::SlowmodeStarted { channel_id, wait });
				}
			}
			Internal::UploadProgress { nonce, uploaded } => self.on_upload_progress(nonce, uploaded),
			Internal::Search(results) => {
				*self.search.lock() = Some(Arc::new(results));
				self.notifier.push(Changes {
					search: true,
					..Changes::default()
				});
			}
			Internal::DmOpened(channel) => {
				let channel_id = channel.id;
				self.apply_dispatch(&DispatchEvent::ChannelCreate(channel));
				self.select_channel(None, channel_id);
			}
			Internal::ThreadOpened(thread) => self.on_thread_opened(thread),
			Internal::SelectJoinedGuild(guild_id) => {
				self.select_created_guild = true;
				if self.state.read().guild(guild_id).is_some() {
					self.select_new_guild(guild_id);
				}
			}
			Internal::Loaded(loaded) => self.on_loaded(loaded),
			Internal::Activities(activities) => {
				self.activities = activities;
				self.publish_presence();
			}
			Internal::PluginApplied { loaded, answer } => self.on_plugin_applied(loaded, answer),
			Internal::Rejected(reason) => self.reject(reason),
			Internal::CacheOpened { user_id, db } => self.on_cache_opened(user_id, db),
			Internal::RestoreRead(snapshots) => {
				for snapshot in snapshots {
					self.apply_dispatch(&DispatchEvent::MessageAck(MessageAck {
						channel_id: snapshot.channel_id,
						message_id: snapshot.message_id,
						version: None,
						mention_count: snapshot.mention_count,
					}));
					self.persist_read_state(snapshot.channel_id);
				}
			}
			Internal::RestoreChannels(placements) => {
				let restored: Vec<Channel> = {
					let state = self.state.read();
					placements
						.into_iter()
						.filter_map(|(id, position, parent_id)| {
							let mut channel = state.channel(id)?.clone();
							channel.position = position;
							channel.parent_id = parent_id;
							Some(channel)
						})
						.collect()
				};
				for channel in restored {
					self.apply_dispatch(&DispatchEvent::ChannelUpdate(Box::new(channel)));
				}
			}
			Internal::FriendRequestSent(username) => {
				self.apply_local(LocalEvent::FriendRequestSent { username })
			}
			Internal::FriendRequestFailed { username, reason } => {
				self.apply_local(LocalEvent::FriendRequestFailed { username, reason })
			}
			Internal::SignedOut => self.on_signed_out(),
			Internal::CameraStarted { generation, result } => self.on_camera_started(generation, result),
			Internal::StreamTimeout { generation } => self.on_stream_timeout(generation),
			Internal::StreamConnected { generation, result } => self.on_stream_connected(generation, result),
			Internal::StreamVideo { generation, result } => self.on_stream_video(generation, result),
			Internal::StreamEvent { generation, event } => self.on_stream_event(generation, event),
			Internal::Draft { channel_id, content } => {
				if self.state.read().draft(channel_id).is_none() {
					self.apply_local(LocalEvent::DraftChanged { channel_id, content });
				}
			}
			Internal::JoinVoice { guild_id, channel_id } => self.join_voice(guild_id, channel_id),
			Internal::VoiceJoinTimeout(generation) => self.on_voice_join_timeout(generation),
			Internal::VoiceSettled(generation) => self.on_voice_settled(generation),
			Internal::VoiceConnected { generation, result } => self.on_voice_connected(generation, result),
			Internal::VoiceEvent { generation, event } => self.on_voice_event(generation, event),
			Internal::VoiceLevels {
				generation,
				input,
				output,
			} => self.on_voice_levels(generation, input, output),
			Internal::Rendered(results) => self.on_rendered(results),
			Internal::PluginsChanged => self.on_plugins_changed(),
			Internal::SurfacesChanged { rerender } => self.on_surfaces_changed(rerender),
			Internal::SlotsChanged => self.on_slots_changed(),
			Internal::CatalogChanged => self.on_catalog_changed(),
			Internal::BeforeSend {
				nonce,
				channel_id,
				outgoing,
				decision,
			} => self.on_before_send(nonce, channel_id, outgoing, decision),
			Internal::AlertVerdict { id, verdict } => self.on_alert_verdict(id, verdict),
			Internal::AutocompleteOrder { generation, order } => {
				self.on_autocomplete_order(generation, order)
			}
			Internal::Decorated { generation, entries } => self.on_decorated(generation, entries),
			Internal::Overrides { epoch, entries } => self.on_overrides(epoch, entries),
			Internal::MicTestLevel {
				generation,
				level,
				remaining_secs,
			} => self.on_mic_test_level(generation, level, remaining_secs),
			Internal::MicTestFinished { generation, result } => self.on_mic_test_finished(generation, result),
			Internal::AccountSwitch(user_id) => self.apply_local(LocalEvent::AccountSwitchRequested(user_id)),
			Internal::Invalidate { plugin, target } => self.on_invalidate(&plugin, target),
			Internal::Command(command) => {
				self.on_command(*command);
			}
			Internal::Intent(intent) => self.push_intent(intent),
		}
	}

	fn on_messages(
		&mut self,
		channel_id: Snowflake,
		direction: LoadDirection,
		messages: Vec<Message>,
		reached_start: bool,
		cache: bool,
	) {
		let ids: Vec<Snowflake> = if cache {
			messages.iter().map(|message| message.id).collect()
		} else {
			Vec::new()
		};
		self.apply(Event::MessagesLoaded {
			channel_id,
			messages,
			direction,
			reached_start,
		});
		if !cache {
			return;
		}
		let stored: Vec<Arc<Message>> = self
			.state
			.read()
			.messages(channel_id)
			.map(|cached| {
				ids.iter()
					.filter_map(|id| cached.by_id.get(id).cloned())
					.collect()
			})
			.unwrap_or_default();
		if !stored.is_empty() {
			self.persist.submit(Job::Messages {
				channel_id,
				messages: stored,
			});
		}
	}

	fn on_command(&mut self, command: Command) -> bool {
		match command {
			Command::SelectChannel { guild_id, channel_id } => self.select_channel(guild_id, channel_id),
			Command::LoadOlder { channel_id } => self.load_older(channel_id),
			Command::LoadNewer { channel_id } => self.load_newer(channel_id),
			Command::LoadAround {
				channel_id,
				message_id,
			} => self.load_page(
				channel_id,
				None,
				LoadDirection::Around,
				page_query(None, None, Some(message_id)),
			),
			Command::LoadLatest { channel_id } => self.load_latest(channel_id),
			Command::SendMessage {
				channel_id,
				content,
				reply_to,
				files,
				stickers,
				by_plugin,
				silent,
			} => self.send_message(
				channel_id,
				Outgoing {
					content,
					reply_to,
					files,
					stickers,
					silent,
				},
				by_plugin,
				None,
			),
			Command::RetryPending { nonce } => self.retry_pending(nonce),
			Command::DiscardPending { nonce } => self.discard_pending(nonce),
			Command::SendAsUser {
				plugin_id,
				channel_id,
				content,
				reply_to,
			} => self.send_message(
				channel_id,
				Outgoing {
					reply_to,
					..Outgoing::plain(content)
				},
				None,
				Some(plugin_id),
			),
			Command::EditMessage {
				channel_id,
				message_id,
				content,
				keep,
			} => self.spawn_action("edit message", move |rest| async move {
				let body = EditMessage {
					content: Some(content),
					attachments: keep.map(|ids| {
						ids.into_iter()
							.map(|id| AttachmentDescriptor {
								id,
								..AttachmentDescriptor::default()
							})
							.collect()
					}),
					..EditMessage::default()
				};
				rest.edit_message(channel_id, message_id, body).await.map(drop)
			}),
			Command::SuppressEmbeds {
				channel_id,
				message_id,
			} => {
				let flags = self
					.state
					.read()
					.messages(channel_id)
					.and_then(|cache| cache.by_id.get(&message_id))
					.map_or_else(MessageFlags::empty, |message| message.flags)
					| MessageFlags::SUPPRESS_EMBEDS;
				self.spawn_action("suppress embeds", move |rest| async move {
					let body = EditMessage {
						flags: Some(flags),
						..EditMessage::default()
					};
					rest.edit_message(channel_id, message_id, body).await.map(drop)
				});
			}
			Command::DeleteMessage {
				channel_id,
				message_id,
			} => {
				if self.delete_allowed(channel_id, message_id) {
					self.spawn_action("delete message", move |rest| async move {
						rest.delete_message(channel_id, message_id).await
					});
				}
			}
			Command::AddReaction {
				channel_id,
				message_id,
				emoji,
				burst,
			} => {
				if self.react_allowed(channel_id, message_id, &emoji) {
					self.remember_reaction(&emoji);
					self.spawn_action("add reaction", move |rest| async move {
						if burst {
							rest.add_burst_reaction(channel_id, message_id, &emoji).await
						} else {
							rest.add_reaction(channel_id, message_id, &emoji).await
						}
					});
				}
			}
			Command::RemoveReaction {
				channel_id,
				message_id,
				emoji,
				burst,
			} => self.spawn_action("remove reaction", move |rest| async move {
				rest.remove_own_reaction(channel_id, message_id, &emoji, burst)
					.await
			}),
			Command::Typing { channel_id } => self.typing(channel_id),
			Command::AckChannel {
				channel_id,
				message_id,
			} => self.ack(channel_id, message_id),
			Command::SetDraft { channel_id, content } => {
				self.persist.submit(Job::Draft {
					channel_id,
					content: content.clone(),
				});
				self.forward_draft(channel_id, &content);
				self.apply_local(LocalEvent::DraftChanged { channel_id, content });
			}
			Command::SetPresence {
				status,
				custom_status,
			} => self.set_presence_command(status, custom_status),
			Command::SetAfk(afk) => self.apply_local(LocalEvent::AfkChanged(afk)),
			Command::ActivitySettingsChanged => self.push_activity_rules(),
			Command::SubscribeGuild { guild_id } => {
				let channel_id = self.subscription_channel(guild_id);
				self.subscribe(guild_id, channel_id);
				self.schedule_overrides(Duration::ZERO);
			}
			Command::MemberListRange {
				guild_id,
				channel_id,
				visible,
			} => {
				if self.member_channel == Some(channel_id) {
					self.member_ranges = member_ranges(visible);
					self.subscribe(guild_id, Some(channel_id));
				}
			}
			Command::SearchMembers { guild_id, query } => {
				if let Err(error) =
					self.gateway
						.request_members(guild_id, &query, MEMBER_SEARCH_LIMIT, Vec::new())
				{
					tracing::warn!(%error, "member search was not sent");
				}
			}
			Command::RefreshPresence { user_id, guild_id } => self.refresh_presence(user_id, guild_id),
			Command::LoadApplicationIcon { application_id } => {
				self.spawn_load("application icon", move |rest| async move {
					let icon = match rest.application_rpc(application_id).await {
						Ok(raw) => raw.get("icon").and_then(Value::as_str).map(str::to_owned),
						Err(error) => {
							tracing::debug!(%error, "application has no public icon");
							None
						}
					};
					Ok(Loaded::ApplicationIcon { application_id, icon })
				});
			}
			Command::OpenDm { user_id } => self.open_dm(user_id),
			Command::Search {
				guild_id,
				channel_id,
				query,
				request,
			} => self.search(guild_id, channel_id, query, request),
			Command::ToggleCategory { category_id, .. } => {
				let collapsed = !self.state.read().is_collapsed(category_id);
				self.apply_local(LocalEvent::CategoryCollapsed {
					channel_id: category_id,
					collapsed,
				});
			}
			Command::ToggleBlockedRun { channel_id, first } => {
				self.apply_local(LocalEvent::BlockedRunToggled { channel_id, first });
			}
			Command::JoinVoice { guild_id, channel_id } => self.join_voice(guild_id, channel_id),
			Command::LeaveVoice => self.leave_voice(),
			Command::SetSelfMute(muted) => self.set_self_mute(muted),
			Command::SetSelfDeaf(deafened) => self.set_self_deaf(deafened),
			Command::SetVoiceDevices { input, output } => self.set_voice_devices(input, output),
			Command::SetUserVolume { user_id, volume } => self.set_user_volume(user_id, volume),
			Command::SetInputMode(mode) => self.set_input_mode(mode),
			Command::SetPushToTalk(transmitting) => self.set_push_to_talk(transmitting),
			Command::SetInputVolume(volume) => self.set_input_volume(volume),
			Command::SetOutputVolume(volume) => self.set_output_volume(volume),
			Command::SetNoiseSuppression(enabled) => self.set_noise_suppression(enabled),
			Command::SetEchoCancellation(enabled) => self.set_echo_cancellation(enabled),
			Command::SetAutoGain(enabled) => self.set_auto_gain(enabled),
			Command::SetVadThreshold { auto, threshold_db } => self.set_vad_threshold(auto, threshold_db),
			Command::SetCameraQuality(quality) => self.set_camera_quality(quality),
			Command::StartMicTest { seconds } => self.start_mic_test(seconds),
			Command::StopMicTest => self.stop_mic_test(),
			Command::RequestToSpeak {
				guild_id,
				channel_id,
				request,
			} => self.request_to_speak(guild_id, channel_id, request),
			Command::SwitchAccount(user_id) => self.switch_account(user_id),
			Command::StartCall { channel_id } => self.start_call(channel_id),
			Command::DeclineCall { channel_id } => self.decline_call(channel_id),
			Command::PluginLoad(load) => self.on_plugin_load(load),
			command @ (Command::StartReply { .. }
			| Command::StartEdit { .. }
			| Command::JumpToMessage { .. }
			| Command::JumpToTop { .. }
			| Command::OpenSettings { .. }
			| Command::Navigate(_)
			| Command::PickFiles { .. }
			| Command::AttachFiles { .. }
			| Command::AttachSticker { .. }
			| Command::SetComposerText { .. }
			| Command::SaveAttachment { .. }
			| Command::SaveFile { .. }
			| Command::SetClientOption { .. }
			| Command::SetDmSections { .. }
			| Command::SetNotificationSettings { .. }
			| Command::CloneEmoji { .. }
			| Command::CloneSticker { .. }) => self.on_plugin_action(command),
			command @ (Command::RunPluginCommand { .. }
			| Command::PluginUiAction { .. }
			| Command::PluginViewEvent { .. }
			| Command::PluginMenuItem { .. }
			| Command::PluginRenderView { .. }
			| Command::PluginEmojiPick { .. }
			| Command::PluginAutocompleteEmoji { .. }
			| Command::PluginMessageClick(_)
			| Command::PluginChannelClick(_)
			| Command::PluginLinkClick { .. }
			| Command::PluginKeybind { .. }
			| Command::PluginBadgeClick { .. }
			| Command::PluginEnable(_)
			| Command::PluginDisable(_)
			| Command::PluginReload(_)
			| Command::PluginInstall { .. }
			| Command::PluginUninstall(_)
			| Command::PluginGrant { .. }
			| Command::PluginSetSetting { .. }
			| Command::PluginsChanged
			| Command::CatalogRefresh { .. }
			| Command::CatalogInstall { .. }
			| Command::ThemeCatalogRefresh { .. }
			| Command::ThemeInstall { .. }) => self.on_plugin_command(command),
			Command::Reconnect => self.gateway.reconnect(),
			Command::Logout => {
				self.store.tokens.clear();
				self.forget_account();
				self.persist.wipe();
				self.rest.clear_token();
				return false;
			}
			Command::Shutdown => return false,
			command => self.on_extra(command),
		}
		true
	}

	fn spawn_rest<F, Fut>(&self, what: &'static str, call: F)
	where
		F: FnOnce(RestClient) -> Fut,
		Fut: Future<Output = rest::Result<()>> + Send + 'static,
	{
		let request = call(self.rest.clone());
		self.runtime.spawn(async move {
			if let Err(error) = request.await {
				tracing::warn!(%error, what, "rest call failed");
			}
		});
	}

	fn select_channel(&mut self, guild_id: Option<Snowflake>, channel_id: Snowflake) {
		self.apply_local(LocalEvent::ChannelSelected { guild_id, channel_id });
		self.remember_selection(guild_id, channel_id);
		self.load_commands(guild_id, channel_id);
		self.render_worker.select(channel_id);
		self.rerender_stale(channel_id);
		self.forward_channel_switch(guild_id, channel_id);
		self.schedule_overrides(Duration::ZERO);
		if self.member_channel != Some(channel_id) {
			self.member_channel = Some(channel_id);
			self.member_ranges = member_ranges(0..0);
		}
		if let Some(guild_id) = guild_id {
			self.subscribe(guild_id, Some(channel_id));
		}
		if self.selected_guild != guild_id {
			self.schedule_decorations();
			if let Some(guild_id) = guild_id {
				self.request_channel_info(guild_id);
			}
		}
		self.selected_guild = guild_id;
		let (has_messages, stale, has_draft) = {
			let state = self.state.read();
			let cache = state.messages(channel_id);
			(
				cache.is_some_and(|cache| !cache.ordered.is_empty()),
				cache.is_some_and(|cache| cache.is_stale()),
				state.draft(channel_id).is_some(),
			)
		};
		if stale {
			self.load_latest(channel_id);
		} else if !has_messages {
			self.load_initial(channel_id);
		}
		if !has_draft {
			self.load_draft(channel_id);
		}
		let latest = self
			.state
			.read()
			.channel(channel_id)
			.and_then(|channel| channel.last_message_id);
		if let Some(message_id) = latest {
			self.ack(channel_id, message_id);
		}
	}

	fn load_initial(&self, channel_id: Snowflake) {
		let rest = self.rest.clone();
		let db = self.persist.database();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let cached = match db {
				Some(db) => tokio::task::spawn_blocking(move || persist::load_messages(&db, channel_id))
					.await
					.unwrap_or_default(),
				None => Vec::new(),
			};
			let had_cache = !cached.is_empty();
			if had_cache {
				let _ = internal.send(Internal::Messages {
					channel_id,
					direction: LoadDirection::History,
					messages: cached,
					reached_start: false,
					cache: false,
				});
			}
			match rest.messages(channel_id, page_query(None, None, None)).await {
				Ok(messages) => {
					let messages = messages.into_vec();
					let _ = internal.send(Internal::Messages {
						channel_id,
						direction: if had_cache {
							LoadDirection::Around
						} else {
							LoadDirection::History
						},
						reached_start: messages.len() < usize::from(PAGE_SIZE),
						messages,
						cache: true,
					});
				}
				Err(error) => tracing::warn!(%error, "could not load messages"),
			}
		});
	}

	fn load_draft(&self, channel_id: Snowflake) {
		let Some(db) = self.persist.database() else {
			return;
		};
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let draft = tokio::task::spawn_blocking(move || persist::load_draft(&db, channel_id))
				.await
				.ok()
				.flatten();
			if let Some(content) = draft {
				let _ = internal.send(Internal::Draft { channel_id, content });
			}
		});
	}

	fn load_older(&mut self, channel_id: Snowflake) {
		let before = {
			let state = self.state.read();
			let Some(cache) = state.messages(channel_id) else {
				return;
			};
			if cache.reached_start {
				return;
			}
			cache.ordered.first().map(|message| message.id)
		};
		self.load_page(
			channel_id,
			Some(Page::Older),
			LoadDirection::History,
			page_query(before, None, None),
		);
	}

	fn load_latest(&mut self, channel_id: Snowflake) {
		self.load_page(
			channel_id,
			Some(Page::Latest),
			LoadDirection::Around,
			page_query(None, None, None),
		);
	}

	fn catch_up(&mut self) {
		let channel = {
			let state = self.state.read();
			state.selected().and_then(|selected| {
				state
					.messages(selected.channel_id)
					.is_some_and(|cache| !cache.ordered.is_empty())
					.then_some(selected.channel_id)
			})
		};
		if let Some(channel_id) = channel {
			self.load_latest(channel_id);
		}
	}

	fn load_newer(&mut self, channel_id: Snowflake) {
		let after = {
			let state = self.state.read();
			let Some(cache) = state.messages(channel_id) else {
				return;
			};
			if !cache.is_detached() {
				return;
			}
			cache.newest_id()
		};
		self.load_page(
			channel_id,
			Some(Page::Newer),
			LoadDirection::Newer,
			page_query(None, after, None),
		);
	}

	fn load_page(
		&mut self,
		channel_id: Snowflake,
		page: Option<Page>,
		direction: LoadDirection,
		query: MessagesQuery,
	) {
		if let Some(page) = page
			&& !self.loading.insert((channel_id, page))
		{
			return;
		}
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let loaded = rest.messages(channel_id, query).await;
			if let Some(page) = page {
				let _ = internal.send(Internal::LoadFinished(channel_id, page));
			}
			match loaded {
				Ok(messages) => {
					let messages = messages.into_vec();
					let _ = internal.send(Internal::Messages {
						channel_id,
						direction,
						reached_start: messages.len() < usize::from(PAGE_SIZE),
						messages,
						cache: true,
					});
				}
				Err(error) => tracing::warn!(%error, "could not load messages"),
			}
		});
	}

	fn next_nonce(&mut self) -> Nonce {
		self.nonce_sequence = self.nonce_sequence.wrapping_add(1);
		let elapsed = (Utc::now().timestamp_millis() - DISCORD_EPOCH_MS).max(0) as u64;
		let sequence = self.nonce_sequence & ((1 << NONCE_SEQUENCE_BITS) - 1);
		Nonce(((elapsed << NONCE_SEQUENCE_BITS) | sequence).to_string())
	}

	fn current_user(&self) -> Option<User> {
		self.state.read().current_user().cloned()
	}

	fn send_message(
		&mut self,
		channel_id: Snowflake,
		outgoing: Outgoing,
		by_plugin: Option<String>,
		skip_hook: Option<String>,
	) {
		if !self.send_allowed(channel_id) || !self.within_limits(channel_id, &outgoing) {
			return;
		}
		let Some(author) = self.current_user() else {
			tracing::warn!("message not sent, no signed in user");
			return;
		};
		if by_plugin.is_none() {
			self.remember_message_emoji(&outgoing.content);
		}
		let nonce = self.next_nonce();
		if outgoing.shows_pending() {
			self.apply_local(LocalEvent::PendingMessage {
				channel_id,
				nonce: nonce.clone(),
				content: outgoing.content.clone(),
				files: outgoing.pending_files(),
				author: Box::new(author),
				by_plugin: by_plugin.clone(),
				silent: outgoing.silent,
			});
		}
		if by_plugin.is_none() && self.flags.send && outgoing.has_payload() {
			self.hook_before_send(nonce, channel_id, outgoing, skip_hook);
		} else {
			self.dispatch_send(nonce, channel_id, outgoing);
		}
	}

	fn dispatch_send(&mut self, nonce: Nonce, channel_id: Snowflake, outgoing: Outgoing) {
		if !outgoing.has_payload() {
			return;
		}
		let Outgoing {
			content,
			reply_to,
			files,
			stickers,
			silent,
		} = outgoing;
		let (inline, loose) = if files.is_empty() {
			(stickers, Vec::new())
		} else {
			(Vec::new(), stickers)
		};
		self.note_stickers(&inline);
		let outgoing = Outgoing {
			content,
			reply_to,
			files,
			stickers: inline,
			silent,
		};
		let task = self.spawn_send(&nonce, channel_id, &outgoing);
		self.sending.insert(
			nonce,
			Sending {
				channel_id,
				outgoing,
				task,
			},
		);
		for sticker_id in loose {
			self.send_sticker(channel_id, sticker_id);
		}
	}

	fn typing(&mut self, channel_id: Snowflake) {
		if !self.may_send(channel_id) {
			return;
		}
		let now = StdInstant::now();
		if self
			.typing_sent
			.get(&channel_id)
			.is_some_and(|sent| now.duration_since(*sent) < TYPING_THROTTLE)
		{
			return;
		}
		self.typing_sent.insert(channel_id, now);
		self.spawn_rest("typing", move |rest| async move {
			rest.trigger_typing(channel_id).await
		});
	}

	fn ack_local(&mut self, channel_id: Snowflake, message_id: Snowflake) -> bool {
		let already_read = self
			.state
			.read()
			.read_state(channel_id)
			.is_some_and(|read_state| {
				read_state.last_message_id >= Some(message_id) && read_state.mention_count.unwrap_or(0) == 0
			});
		if already_read {
			return false;
		}
		self.apply_dispatch(&DispatchEvent::MessageAck(MessageAck {
			channel_id,
			message_id,
			version: None,
			mention_count: Some(0),
		}));
		self.persist_read_state(channel_id);
		true
	}

	pub(super) fn ack(&mut self, channel_id: Snowflake, message_id: Snowflake) {
		if !self.ack_local(channel_id, message_id) {
			return;
		}
		let body = json!({ "token": null, "last_viewed": ack_day() });
		self.spawn_rest("ack", move |rest| async move {
			rest.ack_message(channel_id, message_id, body).await.map(drop)
		});
	}

	fn subscription_channel(&self, guild_id: Snowflake) -> Option<Snowflake> {
		let state = self.state.read();
		state
			.selected()
			.filter(|selected| selected.guild_id == Some(guild_id))
			.map(|selected| selected.channel_id)
			.or_else(|| {
				state
					.guild(guild_id)?
					.channels
					.values()
					.find_map(|channel| (channel.kind == ChannelType::GuildText).then_some(channel.id))
			})
	}

	fn subscribe(&self, guild_id: Snowflake, channel_id: Option<Snowflake>) {
		let channels = channel_id
			.map(|channel_id| {
				let ranges = if self.member_channel == Some(channel_id) {
					self.member_ranges.clone()
				} else {
					member_ranges(0..0)
				};
				(channel_id, ranges)
			})
			.into_iter()
			.collect();
		if let Err(error) = self
			.gateway
			.subscribe_guild(guild_id, channels, true, true, false)
		{
			tracing::warn!(%error, "guild subscription was not sent");
		}
	}

	fn request_channel_info(&self, guild_id: Snowflake) {
		if let Err(error) = self.gateway.request_channel_info(guild_id) {
			tracing::warn!(%error, "channel info request was not sent");
		}
	}

	fn subscribe_selected(&self) {
		let selected = self.state.read().selected();
		if let Some(selected) = selected
			&& let Some(guild_id) = selected.guild_id
		{
			self.subscribe(guild_id, Some(selected.channel_id));
			self.request_channel_info(guild_id);
		}
	}

	fn refresh_presence(&self, user_id: Snowflake, preferred: Option<Snowflake>) {
		let guild_id = self.state.read().shared_guild(user_id, preferred);
		let Some(guild_id) = guild_id else {
			return;
		};
		if let Err(error) = self.gateway.request_members(guild_id, "", 1, vec![user_id]) {
			tracing::warn!(%error, "presence refresh was not sent");
		}
	}

	fn open_dm(&self, user_id: Snowflake) {
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			match rest.create_dm(user_id).await {
				Ok(channel) => {
					let _ = internal.send(Internal::DmOpened(Box::new(channel)));
				}
				Err(error) => tracing::warn!(%error, "could not open the conversation"),
			}
		});
	}

	fn search(
		&self,
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		query: SearchQuery,
		request: u64,
	) {
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let mut retries = 0;
			let outcome = loop {
				match rest.search_messages(guild_id, channel_id, query.clone()).await {
					Err(RestError::IndexNotReady { retry_after }) if retries < SEARCH_INDEX_RETRIES => {
						retries += 1;
						tokio::time::sleep(retry_after.min(SEARCH_INDEX_MAX_WAIT)).await;
					}
					other => break other,
				}
			};
			let results = match outcome {
				Ok(response) => SearchResults::from_response(&response),
				Err(RestError::IndexNotReady { .. }) => SearchResults::indexing(),
				Err(error) => {
					let _ = internal.send(rest_failure("search", &error));
					SearchResults::default()
				}
			};
			let _ = internal.send(Internal::Search(SearchResults { request, ..results }));
		});
	}
}
