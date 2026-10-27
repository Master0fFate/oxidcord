use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant as StdInstant};

use chrono::Utc;
use gateway::{GatewayEvent, GatewayHandle};
use parking_lot::Mutex;
use plugin_host::SendDecision;
use plugin_host::api::VoiceInfo;
use protocol::{
	AllowedMentions, Channel, ChannelType, CreateMessage, DISCORD_EPOCH_MS, DispatchEvent, EditMessage,
	Message, MessageAck, MessageReference, MessagesQuery, Nonce, Snowflake, Token, User,
};
use rest::api::SearchQuery;
use rest::{RestClient, UploadFile};
use serde_json::{Value, json};
use state::{Changes, Event, LoadDirection, Loaded, LocalEvent, SharedState, TYPING_TIMEOUT, UserOverride};
use store::Store;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use crate::command::{Command, ReplyTarget};
use crate::config::VoiceIo;
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
mod plugin_link;
mod streams;
mod video;
mod voice_link;

use mic_test::MicTestRun;
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
const MEMBER_LIST_RANGE: [u32; 2] = [0, 99];
const DAY_MILLIS: i64 = 86_400_000;
const NONCE_SEQUENCE_BITS: u32 = 22;
const INITIAL_EPOCH: u64 = 1;

#[derive(Default)]
pub(crate) struct Stats {
	pub(crate) reconnects: AtomicU64,
}

pub(crate) type SharedSearch = Arc<Mutex<Option<Arc<SearchResults>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Page {
	Older,
	Newer,
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
	SendFailed(Nonce),
	Search(SearchResults),
	DmOpened(Box<Channel>),
	ThreadOpened(Box<Channel>),
	Loaded(Loaded),
	Rejected(String),
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
	SlotsChanged,
	CatalogChanged,
	BeforeSend {
		nonce: Nonce,
		channel_id: Snowflake,
		content: String,
		reply_to: Option<ReplyTarget>,
		files: Vec<UploadFile>,
		decision: SendDecision,
	},
	Overrides(Vec<(Option<Snowflake>, Snowflake, Option<UserOverride>)>),
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
	pub(crate) voice_io: VoiceIo,
	pub(crate) shared_voice: SharedVoice,
	pub(crate) video: SharedVideo,
	pub(crate) internal: mpsc::UnboundedSender<Internal>,
	pub(crate) hub: Arc<PluginHub>,
	pub(crate) safe_mode: bool,
	pub(crate) locale: String,
	pub(crate) token: Arc<Token>,
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
	command_scopes: HashSet<Snowflake>,
	typing_expiry: Option<Instant>,
	nonce_sequence: u64,
	voice: VoiceLink,
	video: SharedVideo,
	streams: StreamLink,
	custom_status: Option<CustomStatus>,
	hub: Arc<PluginHub>,
	render_worker: RenderWorker,
	plugin_jobs: mpsc::UnboundedSender<plugin_link::PluginJob>,
	safe_mode: bool,
	locale: String,
	flags: PluginFlags,
	epoch: u64,
	overrides_due: Option<Instant>,
	overrides_busy: bool,
	mic_test: MicTestRun,
	token: Arc<Token>,
	account_saved: bool,
	select_created_guild: bool,
	last_voice_info: Option<VoiceInfo>,
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

fn ack_day() -> i64 {
	(Utc::now().timestamp_millis() - DISCORD_EPOCH_MS) / DAY_MILLIS
}

struct CustomStatus {
	text: String,
	emoji: Option<String>,
}

impl Core {
	pub(crate) fn new(parts: CoreParts) -> Self {
		let persist = Persistor::spawn(parts.store.clone(), &parts.runtime);
		let plugin_jobs = plugin_link::spawn_plugin_jobs(&parts.runtime);
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
			command_scopes: HashSet::new(),
			typing_expiry: None,
			nonce_sequence: 0,
			voice: VoiceLink::new(parts.voice_io, parts.shared_voice),
			video: parts.video,
			streams: StreamLink::default(),
			custom_status: None,
			hub: parts.hub,
			render_worker,
			plugin_jobs,
			safe_mode: parts.safe_mode,
			locale: parts.locale,
			flags: PluginFlags::default(),
			epoch: INITIAL_EPOCH,
			overrides_due: None,
			overrides_busy: false,
			mic_test: MicTestRun::default(),
			token: parts.token,
			account_saved: false,
			select_created_guild: false,
			last_voice_info: None,
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
		self.restore_recents().await;
		let mut gateway_open = true;
		loop {
			let flush_at = self.notifier.deadline();
			let expire_at = self.typing_expiry;
			let overrides_at = self.overrides_due;
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
				() = sleep_until(overrides_at.unwrap_or_else(Instant::now)), if overrides_at.is_some() => {
					self.refresh_overrides();
				}
			}
		}
		self.notifier.flush();
		self.gateway.shutdown();
		self.persist.close().await;
	}

	async fn restore(&mut self) {
		let store = self.store.clone();
		let cached = tokio::task::spawn_blocking(move || {
			(persist::load_ready(&store), persist::load_read_states(&store))
		})
		.await;
		let Ok((Some(mut ready), read_states)) = cached else {
			return;
		};
		if self
			.store
			.accounts
			.current()
			.is_some_and(|current| current != ready.user.id)
		{
			persist::clear_cache(&self.store);
			return;
		}
		ready.session_id.clear();
		ready.resume_gateway_url = None;
		self.apply(Event::Ready(&ready));
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

	fn restore_selection(&mut self) {
		let Some(saved) = persist::load_value::<SavedSelection>(&self.store, persist::LAST_SELECTION_KEY)
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
		if changes.voice_session {
			self.forward_voice();
		}
		if !changes.typing.is_empty() {
			self.typing_expiry = Some(Instant::now() + TYPING_TIMEOUT + TYPING_EXPIRY_SLACK);
		}
		if changes.own_status {
			self.publish_presence();
		}
		self.notifier.push(changes);
	}

	fn apply_local(&mut self, event: LocalEvent) {
		self.apply(Event::Local(event));
	}

	fn apply_dispatch(&mut self, event: &DispatchEvent) {
		self.apply(Event::Dispatch(event));
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
			GatewayEvent::Hello { .. } => {}
			GatewayEvent::Ready(ready) => {
				self.apply(Event::Ready(&ready));
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
				self.persist.submit(Job::Ready(ready));
			}
			GatewayEvent::ReadySupplemental(supplemental) => {
				self.apply(Event::ReadySupplemental(&supplemental));
			}
			GatewayEvent::Resumed => {
				self.apply_dispatch(&DispatchEvent::Resumed);
				self.resend_voice_state();
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
			GatewayEvent::Fatal(error) => {
				tracing::error!(%error, "gateway stopped");
				self.apply_local(LocalEvent::ConnectionChanged { connected: false });
			}
			GatewayEvent::Latency { heartbeat_rtt } => {
				self.apply_local(LocalEvent::LatencyMeasured(heartbeat_rtt));
			}
			GatewayEvent::DecodeFailure { event } => {
				tracing::debug!(event, "gateway event could not be decoded");
			}
		}
	}

	fn on_dispatch(&mut self, event: DispatchEvent) {
		self.apply_dispatch(&event);
		self.forward_dispatch(&event);
		match event {
			DispatchEvent::MessageCreate(message) => {
				let channel_id = message.channel_id;
				self.persist.submit(Job::Messages {
					channel_id,
					messages: vec![Arc::from(message)],
				});
				self.persist_read_state(channel_id);
			}
			DispatchEvent::MessageUpdate(update) => self.persist_cached_message(update.channel_id, update.id),
			DispatchEvent::MessageReactionAdd(reaction) => {
				self.persist_cached_message(reaction.channel_id, reaction.message_id);
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
			DispatchEvent::MessageDelete(delete) => {
				self.persist.submit(Job::DeleteMessages(vec![delete.id]));
			}
			DispatchEvent::MessageDeleteBulk(delete) => {
				self.persist.submit(Job::DeleteMessages(delete.ids));
			}
			DispatchEvent::MessageAck(ack) => self.persist_read_state(ack.channel_id),
			DispatchEvent::VoiceStateUpdate(update) => self.voice_state_update(&update),
			DispatchEvent::VoiceServerUpdate(update) => self.voice_server_update(update),
			DispatchEvent::StreamCreate(stream) => self.stream_created(&stream),
			DispatchEvent::StreamServerUpdate(update) => self.stream_server_updated(update),
			DispatchEvent::StreamDelete(delete) => self.stream_deleted(&delete),
			DispatchEvent::ChannelPinsUpdate(update) => self.reload_pins(update.channel_id),
			DispatchEvent::GuildCreate(guild) => self.select_new_guild(guild.id),
			_ => {}
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
			} => self.on_messages(channel_id, direction, messages, reached_start, cache),
			Internal::LoadFinished(channel_id, page) => {
				self.loading.remove(&(channel_id, page));
			}
			Internal::SendFailed(nonce) => {
				self.apply_local(LocalEvent::PendingFailed { nonce, reason: None })
			}
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
			Internal::Loaded(loaded) => self.on_loaded(loaded),
			Internal::Rejected(reason) => self.reject(reason),
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
			Internal::VoiceConnected { generation, result } => self.on_voice_connected(generation, result),
			Internal::VoiceEvent { generation, event } => self.on_voice_event(generation, event),
			Internal::VoiceLevels {
				generation,
				input,
				output,
			} => self.on_voice_levels(generation, input, output),
			Internal::Rendered(results) => self.on_rendered(results),
			Internal::PluginsChanged => self.on_plugins_changed(),
			Internal::SlotsChanged => self.on_slots_changed(),
			Internal::CatalogChanged => self.on_catalog_changed(),
			Internal::BeforeSend {
				nonce,
				channel_id,
				content,
				reply_to,
				files,
				decision,
			} => self.on_before_send(nonce, channel_id, content, reply_to, files, decision),
			Internal::Overrides(entries) => self.on_overrides(entries),
			Internal::MicTestLevel {
				generation,
				level,
				remaining_secs,
			} => self.on_mic_test_level(generation, level, remaining_secs),
			Internal::MicTestFinished { generation, result } => self.on_mic_test_finished(generation, result),
			Internal::AccountSwitch(user_id) => self.apply_local(LocalEvent::AccountSwitchRequested(user_id)),
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
			Command::SendMessage {
				channel_id,
				content,
				reply_to,
				files,
				by_plugin,
			} => self.send_message(channel_id, content, reply_to, files, by_plugin),
			Command::EditMessage {
				channel_id,
				message_id,
				content,
			} => self.spawn_rest("edit message", move |rest| async move {
				let body = EditMessage {
					content: Some(content),
					..EditMessage::default()
				};
				rest.edit_message(channel_id, message_id, body).await.map(drop)
			}),
			Command::DeleteMessage {
				channel_id,
				message_id,
			} => self.spawn_rest("delete message", move |rest| async move {
				rest.delete_message(channel_id, message_id).await
			}),
			Command::AddReaction {
				channel_id,
				message_id,
				emoji,
			} => {
				self.remember_reaction(&emoji);
				self.spawn_rest("add reaction", move |rest| async move {
					rest.add_reaction(channel_id, message_id, &emoji).await
				});
			}
			Command::RemoveReaction {
				channel_id,
				message_id,
				emoji,
			} => self.spawn_rest("remove reaction", move |rest| async move {
				rest.remove_own_reaction(channel_id, message_id, &emoji).await
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
				self.apply_local(LocalEvent::DraftChanged { channel_id, content });
			}
			Command::SetPresence {
				status,
				custom_status,
			} => self.set_presence_command(status, custom_status),
			Command::SetAfk(afk) => self.apply_local(LocalEvent::AfkChanged(afk)),
			Command::SubscribeGuild { guild_id } => {
				let channel_id = self.subscription_channel(guild_id);
				self.subscribe(guild_id, channel_id);
				self.schedule_overrides(Duration::ZERO);
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
			} => self.search(guild_id, channel_id, query),
			Command::ToggleCategory { category_id, .. } => {
				let collapsed = !self.state.read().is_collapsed(category_id);
				self.apply_local(LocalEvent::CategoryCollapsed {
					channel_id: category_id,
					collapsed,
				});
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
			Command::SetVadThreshold { auto, threshold_db } => self.set_vad_threshold(auto, threshold_db),
			Command::SetDefaultVideoQuality(quality) => self.set_default_video_quality(quality),
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
			command @ (Command::RunPluginCommand { .. }
			| Command::PluginUiAction { .. }
			| Command::PluginViewEvent { .. }
			| Command::PluginMenuItem { .. }
			| Command::PluginRenderView { .. }
			| Command::PluginEmojiPick { .. }
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
				persist::clear_cache(&self.store);
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
		if let Some(guild_id) = guild_id
			&& self.selected_guild != Some(guild_id)
		{
			self.subscribe(guild_id, Some(channel_id));
		}
		self.selected_guild = guild_id;
		let (has_messages, has_draft) = {
			let state = self.state.read();
			(
				state
					.messages(channel_id)
					.is_some_and(|cache| !cache.ordered.is_empty()),
				state.draft(channel_id).is_some(),
			)
		};
		if !has_messages {
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
		let store = self.store.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let cached = tokio::task::spawn_blocking(move || persist::load_messages(&store, channel_id))
				.await
				.unwrap_or_default();
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
		let store = self.store.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let draft = tokio::task::spawn_blocking(move || persist::load_draft(&store, channel_id))
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
		content: String,
		reply_to: Option<ReplyTarget>,
		files: Vec<UploadFile>,
		by_plugin: Option<String>,
	) {
		if !self.send_allowed(channel_id) {
			return;
		}
		let Some(author) = self.current_user() else {
			tracing::warn!("message not sent, no signed in user");
			return;
		};
		if by_plugin.is_none() {
			self.remember_message_emoji(&content);
		}
		let nonce = self.next_nonce();
		self.apply_local(LocalEvent::PendingMessage {
			channel_id,
			nonce: nonce.clone(),
			content: content.clone(),
			author: Box::new(author),
			by_plugin: by_plugin.clone(),
		});
		if by_plugin.is_none() && self.flags.send && !content.is_empty() {
			self.hook_before_send(nonce, channel_id, content, reply_to, files);
		} else {
			self.dispatch_send(nonce, channel_id, content, reply_to, files);
		}
	}

	fn dispatch_send(
		&mut self,
		nonce: Nonce,
		channel_id: Snowflake,
		content: String,
		reply_to: Option<ReplyTarget>,
		files: Vec<UploadFile>,
	) {
		let body = CreateMessage {
			content: (!content.is_empty()).then_some(content),
			nonce: Some(nonce.0.clone()),
			message_reference: reply_to.map(|reply| MessageReference {
				message_id: Some(reply.message),
				channel_id: Some(channel_id),
				fail_if_not_exists: Some(false),
				..MessageReference::default()
			}),
			allowed_mentions: reply_to.filter(|reply| !reply.mention).map(|_| AllowedMentions {
				parse: Some(vec![
					"users".to_owned(),
					"roles".to_owned(),
					"everyone".to_owned(),
				]),
				replied_user: Some(false),
				..AllowedMentions::default()
			}),
			..CreateMessage::default()
		};
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let sent = if files.is_empty() {
				rest.create_message(channel_id, body).await
			} else {
				rest.create_message_with_files(channel_id, body, files).await
			};
			if let Err(error) = sent {
				tracing::warn!(%error, "message was not sent");
				let _ = internal.send(Internal::SendFailed(nonce));
			}
		});
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
			.map(|channel_id| (channel_id, vec![MEMBER_LIST_RANGE]))
			.into_iter()
			.collect();
		if let Err(error) = self
			.gateway
			.subscribe_guild(guild_id, channels, true, true, false)
		{
			tracing::warn!(%error, "guild subscription was not sent");
		}
	}

	fn subscribe_selected(&self) {
		let selected = self.state.read().selected();
		if let Some(selected) = selected
			&& let Some(guild_id) = selected.guild_id
		{
			self.subscribe(guild_id, Some(selected.channel_id));
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

	fn search(&self, guild_id: Option<Snowflake>, channel_id: Option<Snowflake>, query: SearchQuery) {
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			match rest.search_messages(guild_id, channel_id, query).await {
				Ok(response) => {
					let _ = internal.send(Internal::Search(SearchResults::from_response(&response)));
				}
				Err(error) => tracing::warn!(%error, "search failed"),
			}
		});
	}
}
