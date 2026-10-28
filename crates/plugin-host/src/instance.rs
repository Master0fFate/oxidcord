use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use wasmtime::component::Component;
use wasmtime::{Store, Trap, UpdateDeadline};

use crate::api::model::{
	Badge, ChannelClick, ChannelDetail, ClientEvent, ContextDetail, DisplayOverrideFull, EmojiInfo, EmojiRef,
	LinkVerdict, MessageClick, MessageFull, MessageMeta, MessagePatch, MessagePatchExt, NotificationInfo,
	NotificationVerdict, OutgoingFull, PickVerdict, Presence, RichTree, SendDecisionFull, UiContext,
	ViewEvent, ViewReply, ViewTree,
};
use crate::api::{
	DisplayOverride, Event, MdTree, MessageView, OutgoingMessage, SendDecision, SettingChange, UiTree, User,
};
use crate::bindings::Plugin;
use crate::bindings::ext::exports::oxidcord::plugin::{hooks, hooks2};
use crate::config::{HookTimeouts, RuntimeConfig};
use crate::convert::{empty_patch, empty_patch_ext};
use crate::host_state::{CallContext, HostState};
use crate::runtime::{Prepared, RuntimeShared};

const WORKER_STACK_BYTES: usize = 4 * 1024 * 1024;

pub type DisplayRequest = (Option<u64>, User, Option<Presence>);
pub type InterceptItem = (MessageFull, MessageMeta, RichTree);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
	Loaded,
	Disabled,
	Crashed,
	Incompatible,
}

impl PluginState {
	pub fn is_active(self) -> bool {
		matches!(self, Self::Loaded | Self::Crashed)
	}

	fn to_u8(self) -> u8 {
		match self {
			Self::Loaded => 0,
			Self::Disabled => 1,
			Self::Crashed => 2,
			Self::Incompatible => 3,
		}
	}

	fn from_u8(value: u8) -> Self {
		match value {
			0 => Self::Loaded,
			1 => Self::Disabled,
			2 => Self::Crashed,
			_ => Self::Incompatible,
		}
	}
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CallError {
	#[error("handler exceeded its deadline")]
	Timeout,
	#[error("plugin exceeded its memory limit")]
	MemoryLimit,
	#[error("plugin trapped: {0}")]
	Trap(String),
	#[error("plugin init failed: {0}")]
	InitFailed(String),
	#[error("plugin is restarting")]
	Backoff,
	#[error("plugin is disabled")]
	Disabled,
	#[error("plugin queue is full")]
	Busy,
	#[error("plugin stopped")]
	Stopped,
	#[error("plugin is incompatible: {0}")]
	Incompatible(String),
}

pub struct Health {
	state: AtomicU8,
	revision: Arc<AtomicU64>,
	crashes: Mutex<VecDeque<Instant>>,
	total_crashes: AtomicU64,
	calls: AtomicU64,
	last_micros: AtomicU64,
	denied: AtomicU64,
	last_error: Mutex<Option<String>>,
	blocked: Mutex<Blocked>,
}

#[derive(Default)]
struct Blocked {
	finished: Duration,
	since: Option<Instant>,
}

impl Health {
	pub fn new(state: PluginState, revision: Arc<AtomicU64>) -> Self {
		Self {
			state: AtomicU8::new(state.to_u8()),
			revision,
			crashes: Mutex::new(VecDeque::new()),
			total_crashes: AtomicU64::new(0),
			calls: AtomicU64::new(0),
			last_micros: AtomicU64::new(0),
			denied: AtomicU64::new(0),
			last_error: Mutex::new(None),
			blocked: Mutex::new(Blocked::default()),
		}
	}

	pub fn invalidate_hooks(&self) {
		self.revision.fetch_add(1, Ordering::AcqRel);
	}

	pub fn begin_blocking(&self) {
		self.blocked.lock().since = Some(Instant::now());
	}

	pub fn end_blocking(&self) {
		let mut blocked = self.blocked.lock();
		if let Some(since) = blocked.since.take() {
			blocked.finished += since.elapsed();
		}
	}

	pub fn blocked_for(&self) -> Duration {
		let blocked = self.blocked.lock();
		blocked.finished + blocked.since.map_or(Duration::ZERO, |since| since.elapsed())
	}

	pub fn is_blocking(&self) -> bool {
		self.blocked.lock().since.is_some()
	}

	pub fn state(&self) -> PluginState {
		PluginState::from_u8(self.state.load(Ordering::Acquire))
	}

	pub fn set_state(&self, state: PluginState) {
		let before = PluginState::from_u8(self.state.swap(state.to_u8(), Ordering::AcqRel));
		if before.is_active() != state.is_active() {
			self.revision.fetch_add(1, Ordering::AcqRel);
		}
	}

	pub fn total_crashes(&self) -> u64 {
		self.total_crashes.load(Ordering::Relaxed)
	}

	pub fn calls(&self) -> u64 {
		self.calls.load(Ordering::Relaxed)
	}

	pub fn last_ms(&self) -> f64 {
		self.last_micros.load(Ordering::Relaxed) as f64 / 1000.
	}

	pub fn denied(&self) -> u64 {
		self.denied.load(Ordering::Relaxed)
	}

	pub fn record_call(&self, elapsed: Duration) {
		self.calls.fetch_add(1, Ordering::Relaxed);
		let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
		self.last_micros.store(micros, Ordering::Relaxed);
	}

	pub fn mark_incompatible(&self, detail: &str) {
		*self.last_error.lock() = Some(detail.to_owned());
		self.set_state(PluginState::Incompatible);
	}

	pub fn record_denied(&self) {
		self.denied.fetch_add(1, Ordering::Relaxed);
	}

	pub fn last_error(&self) -> Option<String> {
		self.last_error.lock().clone()
	}

	pub fn reset(&self) {
		self.crashes.lock().clear();
		*self.last_error.lock() = None;
	}

	fn record_crash(&self, config: &RuntimeConfig, error: &CallError) -> usize {
		let now = Instant::now();
		let in_window = {
			let mut crashes = self.crashes.lock();
			crashes.push_back(now);
			while crashes
				.front()
				.is_some_and(|oldest| now.duration_since(*oldest) > config.crash_window)
			{
				crashes.pop_front();
			}
			crashes.len()
		};
		self.total_crashes.fetch_add(1, Ordering::Relaxed);
		*self.last_error.lock() = Some(error.to_string());
		if in_window >= config.crash_limit {
			self.set_state(PluginState::Disabled);
		} else {
			self.set_state(PluginState::Crashed);
		}
		in_window
	}
}

fn backoff_for(config: &RuntimeConfig, crashes_in_window: usize) -> Duration {
	let exponent = crashes_in_window.saturating_sub(1).min(16) as u32;
	config
		.restart_backoff
		.saturating_mul(2u32.saturating_pow(exponent))
		.min(config.max_restart_backoff)
}

pub enum CallKind {
	Init,
	Event(Arc<Event>),
	RenderMessage {
		msg: Arc<MessageView>,
		ast: Arc<MdTree>,
	},
	BeforeSend(Arc<OutgoingMessage>),
	FilterMessage(Arc<MessageView>),
	DisplayUsers(Arc<[(Option<u64>, User)]>),
	UiAction {
		id: String,
		payload: String,
	},
	Command {
		name: String,
		args: String,
		channel_id: u64,
	},
	SettingsSchema,
	Setting {
		key: String,
		value: String,
	},
	ViewEvent(ViewEvent, Option<ContextDetail>),
	MenuItem {
		menu: String,
		key: String,
		context: UiContext,
		detail: Option<ContextDetail>,
	},
	RenderView {
		point: String,
		context: UiContext,
		detail: Option<ContextDetail>,
	},
	Intercept {
		msg: Arc<MessageFull>,
		content: Arc<RichTree>,
	},
	Timer {
		id: u64,
		tag: String,
	},
	EmojiPick {
		emoji: EmojiRef,
		channel_id: u64,
	},
	BeforeSendFull(Arc<OutgoingFull>),
	DisplayUsersFull(Arc<[DisplayRequest]>),
	InterceptExt {
		msg: Arc<MessageFull>,
		meta: Arc<MessageMeta>,
		content: Arc<RichTree>,
	},
	InterceptBatchExt(Arc<[InterceptItem]>),
	ClientEvent(Arc<ClientEvent>),
	MessageClick(MessageClick),
	ChannelClick(ChannelClick),
	LinkClick {
		url: String,
		context: UiContext,
	},
	DraftChange {
		channel_id: u64,
		text: String,
	},
	AutocompleteEmoji {
		query: String,
		candidates: Arc<[EmojiInfo]>,
	},
	DecorateChannels(Arc<[ChannelDetail]>),
	Keybind(String),
	Notification(NotificationInfo),
	BadgeClick {
		user_id: u64,
		key: String,
	},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Priority {
	Interactive,
	Background,
}

impl CallKind {
	fn priority(&self) -> Priority {
		match self {
			Self::DisplayUsers(_) | Self::DisplayUsersFull(_) | Self::DecorateChannels(_) => {
				Priority::Background
			}
			_ => Priority::Interactive,
		}
	}

	fn is_measured(&self) -> bool {
		self.priority() == Priority::Interactive && !matches!(self, Self::Init)
	}

	fn is_gesture(&self) -> bool {
		matches!(
			self,
			Self::UiAction { .. }
				| Self::Command { .. }
				| Self::ViewEvent(..)
				| Self::MenuItem { .. }
				| Self::Keybind(_)
				| Self::BadgeClick { .. }
				| Self::MessageClick(_)
				| Self::ChannelClick(_)
				| Self::LinkClick { .. }
		)
	}

	fn is_hooks2(&self) -> bool {
		matches!(
			self,
			Self::BeforeSendFull(_)
				| Self::DisplayUsersFull(_)
				| Self::InterceptExt { .. }
				| Self::InterceptBatchExt(_)
				| Self::ClientEvent(_)
				| Self::MessageClick(_)
				| Self::ChannelClick(_)
				| Self::LinkClick { .. }
				| Self::DraftChange { .. }
				| Self::AutocompleteEmoji { .. }
				| Self::DecorateChannels(_)
				| Self::Keybind(_)
				| Self::Notification(_)
				| Self::BadgeClick { .. }
		)
	}

	fn detail(&self) -> Option<ContextDetail> {
		match self {
			Self::ViewEvent(_, detail) | Self::MenuItem { detail, .. } | Self::RenderView { detail, .. } => {
				detail.clone()
			}
			_ => None,
		}
	}

	fn context(&self) -> CallContext {
		match self {
			Self::Init => CallContext::Init,
			Self::Timer { .. } => CallContext::Timer,
			Self::RenderMessage { .. } | Self::FilterMessage(_) | Self::RenderView { .. } => {
				CallContext::Render
			}
			Self::Intercept { .. }
			| Self::InterceptExt { .. }
			| Self::InterceptBatchExt(_)
			| Self::BeforeSend(_)
			| Self::BeforeSendFull(_)
			| Self::EmojiPick { .. }
			| Self::Setting { .. }
			| Self::SettingsSchema => CallContext::Intercept,
			Self::DisplayUsers(_) | Self::DisplayUsersFull(_) => CallContext::Display,
			Self::DecorateChannels(_) => CallContext::Decorate,
			Self::AutocompleteEmoji { .. } => CallContext::Autocomplete,
			Self::Notification(_) => CallContext::Notification,
			_ if self.is_gesture() => CallContext::UserAction,
			_ => CallContext::Event,
		}
	}

	pub(crate) fn timeout(&self, timeouts: &HookTimeouts) -> Duration {
		match self {
			Self::Init => timeouts.init,
			Self::Event(_) => timeouts.on_event,
			Self::RenderMessage { .. } => timeouts.render_message,
			Self::BeforeSend(_) => timeouts.before_send,
			Self::FilterMessage(_) => timeouts.filter_message,
			Self::DisplayUsers(batch) => {
				timeouts.display_user + timeouts.display_item * u32::try_from(batch.len()).unwrap_or(u32::MAX)
			}
			Self::UiAction { .. } => timeouts.ui_action,
			Self::Command { .. } => timeouts.command,
			Self::SettingsSchema => timeouts.settings_schema,
			Self::Setting { .. } => timeouts.on_event,
			Self::ViewEvent(..) | Self::MenuItem { .. } => timeouts.view_event,
			Self::RenderView { .. } => timeouts.render_view,
			Self::Intercept { .. } => timeouts.intercept_message,
			Self::Timer { .. } => timeouts.timer,
			Self::EmojiPick { .. } => timeouts.emoji_pick,
			Self::BeforeSendFull(_) => timeouts.before_send_full,
			Self::DisplayUsersFull(batch) => {
				timeouts.display_user_full
					+ timeouts.display_item * u32::try_from(batch.len()).unwrap_or(u32::MAX)
			}
			Self::InterceptExt { .. } => timeouts.intercept_message_ext,
			Self::InterceptBatchExt(items) => (timeouts.intercept_batch_item
				* u32::try_from(items.len()).unwrap_or(u32::MAX))
			.min(timeouts.intercept_batch_cap),
			Self::ClientEvent(_) => timeouts.client_event,
			Self::MessageClick(_) => timeouts.message_click,
			Self::ChannelClick(_) => timeouts.channel_click,
			Self::LinkClick { .. } => timeouts.link_click,
			Self::DraftChange { .. } => timeouts.draft_change,
			Self::AutocompleteEmoji { .. } => timeouts.autocomplete_emoji,
			Self::DecorateChannels(channels) => {
				timeouts.decorate_channel
					+ timeouts.decorate_item * u32::try_from(channels.len()).unwrap_or(u32::MAX)
			}
			Self::Keybind(_) => timeouts.keybind,
			Self::Notification(_) => timeouts.notification,
			Self::BadgeClick { .. } => timeouts.badge_click,
		}
	}
}

pub enum Output {
	Done,
	Render(Option<MdTree>),
	Send(SendDecision),
	Filter(bool),
	Displays(Vec<Option<DisplayOverride>>),
	Ui(UiTree),
	Command(Result<(), String>),
	Schema(String),
	Reply(ViewReply),
	View(Option<ViewTree>),
	Patch(Box<MessagePatch>),
	Pick(PickVerdict),
	SendFull(SendDecisionFull),
	DisplaysFull(Vec<Option<DisplayOverrideFull>>),
	PatchExt(Box<MessagePatchExt>),
	PatchesExt(Vec<MessagePatchExt>),
	Link(LinkVerdict),
	Emojis(Vec<u32>),
	Badges(Vec<Vec<Badge>>),
	Notification(NotificationVerdict),
}

struct Reply {
	sender: SyncSender<Result<Output, CallError>>,
	started: Arc<AtomicBool>,
	cancelled: Arc<AtomicBool>,
}

struct Call {
	kind: CallKind,
	timeout: Duration,
	reply: Option<Reply>,
}

struct QueueState {
	interactive: VecDeque<Call>,
	background: VecDeque<Call>,
	events: VecDeque<Call>,
	timers: HashSet<u64>,
	drafts: HashMap<u64, String>,
	closed: bool,
}

impl QueueState {
	fn release(&mut self, call: &mut Call) {
		match &mut call.kind {
			CallKind::Timer { id, .. } => {
				self.timers.remove(id);
			}
			CallKind::DraftChange { channel_id, text } => {
				if let Some(latest) = self.drafts.remove(channel_id) {
					*text = latest;
				}
			}
			_ => {}
		}
	}

	fn push_event(&mut self, call: Call, cap: usize) {
		if self.events.len() >= cap
			&& let Some(mut oldest) = self.events.pop_front()
		{
			self.release(&mut oldest);
		}
		self.events.push_back(call);
	}
}

struct CallQueue {
	state: Mutex<QueueState>,
	ready: Condvar,
	event_cap: usize,
}

impl CallQueue {
	fn new(event_cap: usize) -> Self {
		Self {
			state: Mutex::new(QueueState {
				interactive: VecDeque::new(),
				background: VecDeque::new(),
				events: VecDeque::new(),
				timers: HashSet::new(),
				drafts: HashMap::new(),
				closed: false,
			}),
			ready: Condvar::new(),
			event_cap,
		}
	}

	fn is_closed(&self) -> bool {
		self.state.lock().closed
	}

	fn push_event(&self, call: Call) -> bool {
		let mut state = self.state.lock();
		if state.closed {
			return false;
		}
		state.push_event(call, self.event_cap);
		drop(state);
		self.ready.notify_one();
		true
	}

	fn push_timer(&self, call: Call) -> bool {
		let CallKind::Timer { id, .. } = &call.kind else {
			return false;
		};
		let mut state = self.state.lock();
		if state.closed || !state.timers.insert(*id) {
			return false;
		}
		state.push_event(call, self.event_cap);
		drop(state);
		self.ready.notify_one();
		true
	}

	fn push_draft(&self, channel_id: u64, text: String, timeout: Duration) -> bool {
		let mut state = self.state.lock();
		if state.closed {
			return false;
		}
		if state.drafts.insert(channel_id, text).is_none() {
			let call = Call {
				kind: CallKind::DraftChange {
					channel_id,
					text: String::new(),
				},
				timeout,
				reply: None,
			};
			state.push_event(call, self.event_cap);
		}
		drop(state);
		self.ready.notify_one();
		true
	}

	fn push_call(&self, call: Call) -> bool {
		let mut state = self.state.lock();
		if state.closed || state.interactive.len() + state.background.len() >= self.event_cap {
			return false;
		}
		match call.kind.priority() {
			Priority::Interactive => state.interactive.push_back(call),
			Priority::Background => state.background.push_back(call),
		}
		drop(state);
		self.ready.notify_one();
		true
	}

	fn pop(&self) -> Option<Call> {
		let mut state = self.state.lock();
		loop {
			if state.closed {
				return None;
			}
			if let Some(call) = state.interactive.pop_front() {
				return Some(call);
			}
			if let Some(call) = state.background.pop_front() {
				return Some(call);
			}
			if let Some(mut call) = state.events.pop_front() {
				state.release(&mut call);
				return Some(call);
			}
			self.ready.wait(&mut state);
		}
	}

	fn close(&self) {
		let mut state = self.state.lock();
		state.closed = true;
		state.interactive.clear();
		state.background.clear();
		state.events.clear();
		state.timers.clear();
		state.drafts.clear();
		drop(state);
		self.ready.notify_all();
	}
}

struct Running {
	store: Store<HostState>,
	plugin: Plugin,
	hooks: Option<hooks::Guest>,
	hooks2: Option<hooks2::Guest>,
}

struct WorkerContext {
	id: Arc<str>,
	runtime: Arc<RuntimeShared>,
	pre: Prepared,
	health: Arc<Health>,
}

fn quiet_reply() -> ViewReply {
	ViewReply {
		replace: None,
		close_overlay: false,
		toast: None,
	}
}

const BLOCKED_POLL: Duration = Duration::from_millis(100);

fn ticks_for(tick: Duration, timeout: Duration) -> u64 {
	timeout.div_duration_f64(tick).ceil() as u64 + 1
}

impl WorkerContext {
	fn epoch_ticks(&self, timeout: Duration) -> u64 {
		ticks_for(self.runtime.config.epoch_tick, timeout)
	}

	fn classify(&self, error: &wasmtime::Error, memory_exceeded: bool) -> CallError {
		if memory_exceeded {
			CallError::MemoryLimit
		} else if matches!(error.downcast_ref::<Trap>(), Some(Trap::Interrupt)) {
			CallError::Timeout
		} else {
			CallError::Trap(format!("{error:#}"))
		}
	}

	fn start(&self) -> Result<Running, CallError> {
		let config = &self.runtime.config;
		self.runtime.services.timers.clear_plugin(&self.id);
		self.runtime.services.keybinds.clear_plugin(&self.id);
		self.runtime.services.watches.clear_plugin(&self.id);
		let state = HostState::new(Arc::clone(&self.id), Arc::clone(&self.runtime.services), config)
			.with_health(Arc::clone(&self.health));
		let mut store = Store::new(&self.runtime.engine, state);
		store.limiter(|state| &mut state.limits);
		let tick = config.epoch_tick;
		store.epoch_deadline_callback(move |mut hook| {
			let blocked = hook.data_mut().take_blocked_time();
			Ok(if blocked.is_zero() {
				UpdateDeadline::Interrupt
			} else {
				UpdateDeadline::Continue(ticks_for(tick, blocked))
			})
		});
		store.set_epoch_deadline(self.epoch_ticks(config.timeouts.init));
		let loaded = self.pre.instance.instantiate(&mut store).and_then(|instance| {
			let plugin = self.pre.guest.load(&mut store, &instance)?;
			Ok((plugin, instance))
		});
		let (plugin, instance) = match loaded {
			Ok(loaded) => loaded,
			Err(error) => return Err(self.classify(&error, store.data().limits.memory_exceeded)),
		};
		let hooks = match &self.pre.hooks {
			Some(indices) => Some(
				indices
					.load(&mut store, &instance)
					.map_err(|error| CallError::Incompatible(format!("hooks: {error:#}")))?,
			),
			None => None,
		};
		let hooks2 = match &self.pre.hooks2 {
			Some(indices) => Some(
				indices
					.load(&mut store, &instance)
					.map_err(|error| CallError::Incompatible(format!("hooks2: {error:#}")))?,
			),
			None => None,
		};
		match plugin.oxidcord_plugin_guest().call_init(&mut store) {
			Ok(Ok(())) => Ok(Running {
				store,
				plugin,
				hooks,
				hooks2,
			}),
			Ok(Err(message)) => Err(CallError::InitFailed(message)),
			Err(error) => Err(self.classify(&error, store.data().limits.memory_exceeded)),
		}
	}

	fn execute(&self, running: &mut Running, kind: CallKind, timeout: Duration) -> Result<Output, CallError> {
		let Running {
			store,
			plugin,
			hooks,
			hooks2,
		} = running;
		store.set_epoch_deadline(self.epoch_ticks(timeout));
		let guest = plugin.oxidcord_plugin_guest();
		let gesture = kind.is_gesture();
		store.data_mut().mark_blocked_time();
		store.data_mut().set_context(kind.context());
		store.data_mut().set_detail(kind.detail());
		if gesture {
			store.data_mut().begin_gesture();
		}
		let result = match kind {
			CallKind::Init => Ok(Output::Done),
			CallKind::Event(event) => guest.call_on_event(&mut *store, &event).map(|()| Output::Done),
			CallKind::RenderMessage { msg, ast } => guest
				.call_render_message(&mut *store, &msg, &ast)
				.map(Output::Render),
			CallKind::BeforeSend(msg) => guest.call_before_send(&mut *store, &msg).map(Output::Send),
			CallKind::FilterMessage(msg) => guest.call_filter_message(&mut *store, &msg).map(Output::Filter),
			CallKind::DisplayUsers(batch) => {
				let ticks = self.epoch_ticks(self.runtime.config.timeouts.display_item);
				batch
					.iter()
					.map(|(guild_id, user)| {
						store.set_epoch_deadline(ticks);
						guest.call_display_user(&mut *store, *guild_id, user)
					})
					.collect::<wasmtime::Result<Vec<_>>>()
					.map(Output::Displays)
			}
			CallKind::UiAction { id, payload } => guest
				.call_on_ui_action(&mut *store, &id, &payload)
				.map(Output::Ui),
			CallKind::Command {
				name,
				args,
				channel_id,
			} => guest
				.call_on_command(&mut *store, &name, &args, channel_id)
				.map(Output::Command),
			CallKind::SettingsSchema => guest.call_settings_schema(&mut *store).map(Output::Schema),
			CallKind::Setting { key, value } => {
				if let Err(error) = store.data_mut().store_setting(&key, &value) {
					tracing::warn!(target: "plugin", plugin = &*self.id, "setting was not stored: {error:?}");
				}
				let event = Event::SettingChanged(SettingChange { key, value });
				guest.call_on_event(&mut *store, &event).map(|()| Output::Done)
			}
			kind if kind.is_hooks2() => self.execute_hook2(store, hooks2.as_ref(), kind),
			kind => execute_hook(store, hooks.as_ref(), kind),
		};
		if gesture {
			store.data_mut().end_gesture();
		}
		result.map_err(|error| self.classify(&error, store.data().limits.memory_exceeded))
	}

	fn execute_hook2(
		&self,
		store: &mut Store<HostState>,
		hooks2: Option<&hooks2::Guest>,
		kind: CallKind,
	) -> wasmtime::Result<Output> {
		let timeouts = &self.runtime.config.timeouts;
		match (hooks2, kind) {
			(Some(hooks), CallKind::BeforeSendFull(msg)) => hooks
				.call_before_send_full(&mut *store, &msg)
				.map(Output::SendFull),
			(None, CallKind::BeforeSendFull(_)) => Ok(Output::SendFull(SendDecisionFull::Pass)),
			(Some(hooks), CallKind::DisplayUsersFull(batch)) => {
				let ticks = self.epoch_ticks(timeouts.display_item);
				batch
					.iter()
					.map(|(guild_id, user, presence)| {
						store.set_epoch_deadline(ticks);
						hooks.call_display_user_full(&mut *store, *guild_id, user, presence.as_ref())
					})
					.collect::<wasmtime::Result<Vec<_>>>()
					.map(Output::DisplaysFull)
			}
			(None, CallKind::DisplayUsersFull(batch)) => Ok(Output::DisplaysFull(vec![None; batch.len()])),
			(Some(hooks), CallKind::InterceptExt { msg, meta, content }) => hooks
				.call_intercept_message_ext(&mut *store, &msg, *meta, &content)
				.map(|patch| Output::PatchExt(Box::new(patch))),
			(None, CallKind::InterceptExt { .. }) => Ok(Output::PatchExt(Box::new(empty_patch_ext()))),
			(Some(hooks), CallKind::InterceptBatchExt(items)) => hooks
				.call_intercept_messages_ext(&mut *store, &items)
				.map(Output::PatchesExt),
			(None, CallKind::InterceptBatchExt(items)) => {
				Ok(Output::PatchesExt(vec![empty_patch_ext(); items.len()]))
			}
			(Some(hooks), CallKind::ClientEvent(event)) => hooks
				.call_on_client_event(&mut *store, &event)
				.map(|()| Output::Done),
			(Some(hooks), CallKind::MessageClick(click)) => hooks
				.call_on_message_click(&mut *store, click)
				.map(|()| Output::Done),
			(Some(hooks), CallKind::ChannelClick(click)) => hooks
				.call_on_channel_click(&mut *store, click)
				.map(|()| Output::Done),
			(Some(hooks), CallKind::LinkClick { url, context }) => hooks
				.call_on_link_click(&mut *store, &url, &context)
				.map(Output::Link),
			(None, CallKind::LinkClick { .. }) => Ok(Output::Link(LinkVerdict::Pass)),
			(Some(hooks), CallKind::DraftChange { channel_id, text }) => hooks
				.call_on_draft_change(&mut *store, channel_id, &text)
				.map(|()| Output::Done),
			(Some(hooks), CallKind::AutocompleteEmoji { query, candidates }) => hooks
				.call_autocomplete_emoji(&mut *store, &query, &candidates)
				.map(Output::Emojis),
			(None, CallKind::AutocompleteEmoji { .. }) => Ok(Output::Emojis(Vec::new())),
			(Some(hooks), CallKind::DecorateChannels(channels)) => {
				let ticks = self.epoch_ticks(timeouts.decorate_item);
				channels
					.iter()
					.map(|channel| {
						store.set_epoch_deadline(ticks);
						hooks.call_decorate_channel(&mut *store, channel)
					})
					.collect::<wasmtime::Result<Vec<_>>>()
					.map(Output::Badges)
			}
			(None, CallKind::DecorateChannels(channels)) => {
				Ok(Output::Badges(vec![Vec::new(); channels.len()]))
			}
			(Some(hooks), CallKind::Keybind(id)) => {
				hooks.call_on_keybind(&mut *store, &id).map(|()| Output::Done)
			}
			(Some(hooks), CallKind::Notification(info)) => hooks
				.call_on_notification(&mut *store, info)
				.map(Output::Notification),
			(None, CallKind::Notification(_)) => Ok(Output::Notification(NotificationVerdict::Allow)),
			(Some(hooks), CallKind::BadgeClick { user_id, key }) => hooks
				.call_on_badge_click(&mut *store, user_id, &key)
				.map(|()| Output::Done),
			_ => Ok(Output::Done),
		}
	}

	fn run(
		&self,
		running: &mut Option<Running>,
		restart_at: &mut Instant,
		kind: CallKind,
		timeout: Duration,
	) -> Result<Output, CallError> {
		if running.is_none() {
			if Instant::now() < *restart_at {
				return Err(CallError::Backoff);
			}
			match self.start() {
				Ok(started) => *running = Some(started),
				Err(CallError::Incompatible(detail)) => {
					self.health.mark_incompatible(&detail);
					return Err(CallError::Incompatible(detail));
				}
				Err(error) => {
					self.record_failure(restart_at, &error);
					return Err(error);
				}
			}
		}
		let Some(active) = running.as_mut() else {
			return Err(CallError::Stopped);
		};
		let started = Instant::now();
		let outcome = self.execute(active, kind, timeout);
		self.health.record_call(started.elapsed());
		match outcome {
			Ok(output) => {
				if self.health.state() == PluginState::Crashed {
					self.health.set_state(PluginState::Loaded);
				}
				Ok(output)
			}
			Err(error) => {
				*running = None;
				self.record_failure(restart_at, &error);
				Err(error)
			}
		}
	}

	fn record_failure(&self, restart_at: &mut Instant, error: &CallError) {
		let config = &self.runtime.config;
		let crashes = self.health.record_crash(config, error);
		*restart_at = Instant::now() + backoff_for(config, crashes);
		tracing::warn!(target: "plugin", plugin = &*self.id, "{error}");
	}
}

fn execute_hook(
	store: &mut Store<HostState>,
	hooks: Option<&hooks::Guest>,
	kind: CallKind,
) -> wasmtime::Result<Output> {
	match (hooks, kind) {
		(Some(hooks), CallKind::ViewEvent(event, _)) => {
			hooks.call_on_view_event(&mut *store, &event).map(Output::Reply)
		}
		(None, CallKind::ViewEvent(..)) => Ok(Output::Reply(quiet_reply())),
		(
			Some(hooks),
			CallKind::MenuItem {
				menu, key, context, ..
			},
		) => hooks
			.call_on_menu_item(&mut *store, &menu, &key, &context)
			.map(|()| Output::Done),
		(Some(hooks), CallKind::RenderView { point, context, .. }) => hooks
			.call_render_view(&mut *store, &point, &context)
			.map(Output::View),
		(None, CallKind::RenderView { .. }) => Ok(Output::View(None)),
		(Some(hooks), CallKind::Intercept { msg, content }) => hooks
			.call_intercept_message(&mut *store, &msg, &content)
			.map(|patch| Output::Patch(Box::new(patch))),
		(None, CallKind::Intercept { .. }) => Ok(Output::Patch(Box::new(empty_patch()))),
		(Some(hooks), CallKind::Timer { id, tag }) => {
			hooks.call_on_timer(&mut *store, id, &tag).map(|()| Output::Done)
		}
		(Some(hooks), CallKind::EmojiPick { emoji, channel_id }) => hooks
			.call_on_emoji_pick(&mut *store, &emoji, channel_id)
			.map(Output::Pick),
		(None, CallKind::EmojiPick { .. }) => Ok(Output::Pick(PickVerdict::Default)),
		_ => Ok(Output::Done),
	}
}

fn worker_main(context: WorkerContext, queue: Arc<CallQueue>) {
	let mut running: Option<Running> = None;
	let mut restart_at = Instant::now();
	while let Some(call) = queue.pop() {
		let Call { kind, timeout, reply } = call;
		if let Some(reply) = &reply {
			if reply.cancelled.load(Ordering::Acquire) {
				continue;
			}
			reply.started.store(true, Ordering::Release);
		}
		let result = {
			let _busy = context.runtime.activity.enter();
			context.run(&mut running, &mut restart_at, kind, timeout)
		};
		if let Some(reply) = reply
			&& reply.sender.send(result).is_err()
			&& context.health.state() == PluginState::Loaded
		{
			context.health.set_state(PluginState::Crashed);
		}
		if context.health.state() == PluginState::Disabled {
			queue.close();
			break;
		}
	}
}

struct Slot {
	queue: Option<Arc<CallQueue>>,
	restart_after: Instant,
}

impl Drop for PluginInstance {
	fn drop(&mut self) {
		self.shutdown();
	}
}

pub struct PluginInstance {
	id: Arc<str>,
	runtime: Arc<RuntimeShared>,
	pre: Prepared,
	health: Arc<Health>,
	slot: Mutex<Slot>,
}

impl PluginInstance {
	pub(crate) fn new(
		id: Arc<str>,
		runtime: Arc<RuntimeShared>,
		component: &Component,
		health: Arc<Health>,
	) -> crate::error::Result<Arc<Self>> {
		let pre = runtime.prepare(component)?;
		Ok(Arc::new(Self {
			id,
			runtime,
			pre,
			health,
			slot: Mutex::new(Slot {
				queue: None,
				restart_after: Instant::now(),
			}),
		}))
	}

	pub fn id(&self) -> &str {
		&self.id
	}

	pub fn health(&self) -> &Arc<Health> {
		&self.health
	}

	pub fn is_ext(&self) -> bool {
		self.pre.hooks.is_some()
	}

	pub fn has_hooks2(&self) -> bool {
		self.pre.hooks2.is_some()
	}

	pub fn start(&self) -> Result<(), CallError> {
		self.health.reset();
		self.health.set_state(PluginState::Loaded);
		self.call(CallKind::Init).map(|_| ())
	}

	pub fn shutdown(&self) {
		let mut slot = self.slot.lock();
		if let Some(queue) = slot.queue.take() {
			queue.close();
		}
	}

	fn queue(&self) -> Result<Arc<CallQueue>, CallError> {
		if matches!(
			self.health.state(),
			PluginState::Disabled | PluginState::Incompatible
		) {
			return Err(CallError::Disabled);
		}
		let mut slot = self.slot.lock();
		if let Some(queue) = &slot.queue
			&& !queue.is_closed()
		{
			return Ok(Arc::clone(queue));
		}
		slot.queue = None;
		if Instant::now() < slot.restart_after {
			return Err(CallError::Backoff);
		}
		let queue = Arc::new(CallQueue::new(self.runtime.config.event_queue_cap));
		let context = WorkerContext {
			id: Arc::clone(&self.id),
			runtime: Arc::clone(&self.runtime),
			pre: self.pre.clone(),
			health: Arc::clone(&self.health),
		};
		let worker_queue = Arc::clone(&queue);
		thread::Builder::new()
			.name(format!("plugin-{}", self.id))
			.stack_size(WORKER_STACK_BYTES)
			.spawn(move || worker_main(context, worker_queue))
			.map_err(|error| CallError::Trap(error.to_string()))?;
		slot.queue = Some(Arc::clone(&queue));
		Ok(queue)
	}

	pub fn post_event(&self, event: Arc<Event>) {
		let Ok(queue) = self.queue() else {
			return;
		};
		let kind = CallKind::Event(event);
		let timeout = kind.timeout(&self.runtime.config.timeouts);
		queue.push_event(Call {
			kind,
			timeout,
			reply: None,
		});
	}

	pub fn post_timer(&self, id: u64, tag: String) {
		let Ok(queue) = self.queue() else {
			return;
		};
		let kind = CallKind::Timer { id, tag };
		let timeout = kind.timeout(&self.runtime.config.timeouts);
		queue.push_timer(Call {
			kind,
			timeout,
			reply: None,
		});
	}

	fn post_interactive(&self, kind: CallKind) {
		if self.pre.hooks2.is_none() {
			return;
		}
		let Ok(queue) = self.queue() else {
			return;
		};
		let timeout = kind.timeout(&self.runtime.config.timeouts);
		queue.push_call(Call {
			kind,
			timeout,
			reply: None,
		});
	}

	pub fn post_client_event(&self, event: Arc<ClientEvent>) {
		if self.pre.hooks2.is_none() {
			return;
		}
		let Ok(queue) = self.queue() else {
			return;
		};
		let kind = CallKind::ClientEvent(event);
		let timeout = kind.timeout(&self.runtime.config.timeouts);
		queue.push_event(Call {
			kind,
			timeout,
			reply: None,
		});
	}

	pub fn post_draft_change(&self, channel_id: u64, text: String) {
		if self.pre.hooks2.is_none() {
			return;
		}
		let Ok(queue) = self.queue() else {
			return;
		};
		queue.push_draft(channel_id, text, self.runtime.config.timeouts.draft_change);
	}

	pub fn post_message_click(&self, click: MessageClick) {
		self.post_interactive(CallKind::MessageClick(click));
	}

	pub fn post_channel_click(&self, click: ChannelClick) {
		self.post_interactive(CallKind::ChannelClick(click));
	}

	pub fn post_keybind(&self, id: String) {
		self.post_interactive(CallKind::Keybind(id));
	}

	pub fn post_badge_click(&self, user_id: u64, key: String) {
		self.post_interactive(CallKind::BadgeClick { user_id, key });
	}

	pub fn call(&self, kind: CallKind) -> Result<Output, CallError> {
		let measured = kind.is_measured();
		let issued = Instant::now();
		let result = self.call_queued(kind);
		if measured && result.is_ok() {
			self.runtime.services.latency.record(issued.elapsed());
		}
		result
	}

	fn call_queued(&self, kind: CallKind) -> Result<Output, CallError> {
		let queue = self.queue()?;
		let timeout = kind.timeout(&self.runtime.config.timeouts);
		let (sender, receiver) = sync_channel(1);
		let started = Arc::new(AtomicBool::new(false));
		let cancelled = Arc::new(AtomicBool::new(false));
		let reply = Reply {
			sender,
			started: Arc::clone(&started),
			cancelled: Arc::clone(&cancelled),
		};
		let call = Call {
			kind,
			timeout,
			reply: Some(reply),
		};
		if !queue.push_call(call) {
			return Err(CallError::Busy);
		}
		let allowance = timeout + self.runtime.config.caller_grace;
		let blocked_before = self.health.blocked_for();
		let issued = Instant::now();
		loop {
			let limit = allowance + self.health.blocked_for().saturating_sub(blocked_before);
			let remaining = limit.saturating_sub(issued.elapsed());
			if remaining.is_zero() {
				cancelled.store(true, Ordering::Release);
				if started.load(Ordering::Acquire) {
					self.abandon(&queue);
				}
				return Err(CallError::Timeout);
			}
			let wait = if self.health.is_blocking() {
				remaining.min(BLOCKED_POLL)
			} else {
				remaining
			};
			match receiver.recv_timeout(wait) {
				Ok(result) => return result,
				Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
				Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
					return Err(if self.health.state() == PluginState::Disabled {
						CallError::Disabled
					} else {
						CallError::Stopped
					});
				}
			}
		}
	}

	fn abandon(&self, queue: &Arc<CallQueue>) {
		let config = &self.runtime.config;
		let crashes = self.health.record_crash(config, &CallError::Timeout);
		queue.close();
		let mut slot = self.slot.lock();
		if slot
			.queue
			.as_ref()
			.is_some_and(|current| Arc::ptr_eq(current, queue))
		{
			slot.queue = None;
		}
		slot.restart_after = Instant::now() + backoff_for(config, crashes);
	}
}

fn mismatch() -> CallError {
	CallError::Trap("unexpected handler output".to_owned())
}

impl PluginInstance {
	pub fn render_message(
		&self,
		msg: Arc<MessageView>,
		ast: Arc<MdTree>,
	) -> Result<Option<MdTree>, CallError> {
		match self.call(CallKind::RenderMessage { msg, ast })? {
			Output::Render(tree) => Ok(tree),
			_ => Err(mismatch()),
		}
	}

	pub fn before_send(&self, msg: Arc<OutgoingMessage>) -> Result<SendDecision, CallError> {
		match self.call(CallKind::BeforeSend(msg))? {
			Output::Send(decision) => Ok(decision),
			_ => Err(mismatch()),
		}
	}

	pub fn filter_message(&self, msg: Arc<MessageView>) -> Result<bool, CallError> {
		match self.call(CallKind::FilterMessage(msg))? {
			Output::Filter(keep) => Ok(keep),
			_ => Err(mismatch()),
		}
	}

	pub fn display_users(
		&self,
		batch: Arc<[(Option<u64>, User)]>,
	) -> Result<Vec<Option<DisplayOverride>>, CallError> {
		match self.call(CallKind::DisplayUsers(batch))? {
			Output::Displays(overrides) => Ok(overrides),
			_ => Err(mismatch()),
		}
	}

	pub fn ui_action(&self, id: String, payload: String) -> Result<UiTree, CallError> {
		match self.call(CallKind::UiAction { id, payload })? {
			Output::Ui(tree) => Ok(tree),
			_ => Err(mismatch()),
		}
	}

	pub fn command(
		&self,
		name: String,
		args: String,
		channel_id: u64,
	) -> Result<Result<(), String>, CallError> {
		match self.call(CallKind::Command {
			name,
			args,
			channel_id,
		})? {
			Output::Command(result) => Ok(result),
			_ => Err(mismatch()),
		}
	}

	pub fn settings_schema(&self) -> Result<String, CallError> {
		match self.call(CallKind::SettingsSchema)? {
			Output::Schema(schema) => Ok(schema),
			_ => Err(mismatch()),
		}
	}

	pub fn set_setting(&self, key: String, value: String) -> Result<(), CallError> {
		self.call(CallKind::Setting { key, value }).map(|_| ())
	}

	pub fn call_event(&self, event: Event) -> Result<(), CallError> {
		self.call(CallKind::Event(Arc::new(event))).map(|_| ())
	}
}

impl PluginInstance {
	pub fn view_event(
		&self,
		event: ViewEvent,
		detail: Option<ContextDetail>,
	) -> Result<ViewReply, CallError> {
		match self.call(CallKind::ViewEvent(event, detail))? {
			Output::Reply(reply) => Ok(reply),
			_ => Err(mismatch()),
		}
	}

	pub fn menu_item(
		&self,
		menu: String,
		key: String,
		context: UiContext,
		detail: Option<ContextDetail>,
	) -> Result<(), CallError> {
		self.call(CallKind::MenuItem {
			menu,
			key,
			context,
			detail,
		})
		.map(|_| ())
	}

	pub fn render_view(
		&self,
		point: String,
		context: UiContext,
		detail: Option<ContextDetail>,
	) -> Result<Option<ViewTree>, CallError> {
		match self.call(CallKind::RenderView {
			point,
			context,
			detail,
		})? {
			Output::View(tree) => Ok(tree),
			_ => Err(mismatch()),
		}
	}

	pub fn intercept(
		&self,
		msg: Arc<MessageFull>,
		content: Arc<RichTree>,
	) -> Result<MessagePatch, CallError> {
		match self.call(CallKind::Intercept { msg, content })? {
			Output::Patch(patch) => Ok(*patch),
			_ => Err(mismatch()),
		}
	}

	pub fn emoji_pick(&self, emoji: EmojiRef, channel_id: u64) -> Result<PickVerdict, CallError> {
		match self.call(CallKind::EmojiPick { emoji, channel_id })? {
			Output::Pick(verdict) => Ok(verdict),
			_ => Err(mismatch()),
		}
	}
}

impl PluginInstance {
	pub fn before_send_full(&self, msg: Arc<OutgoingFull>) -> Result<SendDecisionFull, CallError> {
		match self.call(CallKind::BeforeSendFull(msg))? {
			Output::SendFull(decision) => Ok(decision),
			_ => Err(mismatch()),
		}
	}

	pub fn display_users_full(
		&self,
		batch: Arc<[DisplayRequest]>,
	) -> Result<Vec<Option<DisplayOverrideFull>>, CallError> {
		match self.call(CallKind::DisplayUsersFull(batch))? {
			Output::DisplaysFull(overrides) => Ok(overrides),
			_ => Err(mismatch()),
		}
	}

	pub fn intercept_ext(
		&self,
		msg: Arc<MessageFull>,
		meta: Arc<MessageMeta>,
		content: Arc<RichTree>,
	) -> Result<MessagePatchExt, CallError> {
		match self.call(CallKind::InterceptExt { msg, meta, content })? {
			Output::PatchExt(patch) => Ok(*patch),
			_ => Err(mismatch()),
		}
	}

	pub fn intercept_batch_ext(
		&self,
		items: Arc<[InterceptItem]>,
	) -> Result<Vec<MessagePatchExt>, CallError> {
		match self.call(CallKind::InterceptBatchExt(items))? {
			Output::PatchesExt(patches) => Ok(patches),
			_ => Err(mismatch()),
		}
	}

	pub fn link_click(&self, url: String, context: UiContext) -> Result<LinkVerdict, CallError> {
		match self.call(CallKind::LinkClick { url, context })? {
			Output::Link(verdict) => Ok(verdict),
			_ => Err(mismatch()),
		}
	}

	pub fn autocomplete_emoji(
		&self,
		query: String,
		candidates: Arc<[EmojiInfo]>,
	) -> Result<Vec<u32>, CallError> {
		match self.call(CallKind::AutocompleteEmoji { query, candidates })? {
			Output::Emojis(order) => Ok(order),
			_ => Err(mismatch()),
		}
	}

	pub fn decorate_channels(&self, channels: Arc<[ChannelDetail]>) -> Result<Vec<Vec<Badge>>, CallError> {
		match self.call(CallKind::DecorateChannels(channels))? {
			Output::Badges(badges) => Ok(badges),
			_ => Err(mismatch()),
		}
	}

	pub fn notification(&self, info: NotificationInfo) -> Result<NotificationVerdict, CallError> {
		match self.call(CallKind::Notification(info))? {
			Output::Notification(verdict) => Ok(verdict),
			_ => Err(mismatch()),
		}
	}
}
