use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use wasmtime::component::Component;
use wasmtime::{Store, Trap};

use crate::api::model::{
	EmojiRef, MessageFull, MessagePatch, PickVerdict, RichTree, UiContext, ViewEvent, ViewReply, ViewTree,
};
use crate::api::{
	DisplayOverride, Event, MdTree, MessageView, OutgoingMessage, SendDecision, SettingChange, UiTree, User,
};
use crate::bindings::Plugin;
use crate::bindings::ext::exports::oxidcord::plugin::hooks;
use crate::config::{HookTimeouts, RuntimeConfig};
use crate::convert::empty_patch;
use crate::host_state::HostState;
use crate::runtime::{Prepared, RuntimeShared};

const WORKER_STACK_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
	Loaded,
	Disabled,
	Crashed,
	Incompatible,
}

impl PluginState {
	pub(crate) fn is_active(self) -> bool {
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
		}
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
	ViewEvent(ViewEvent),
	MenuItem {
		menu: String,
		key: String,
		context: UiContext,
	},
	RenderView {
		point: String,
		context: UiContext,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Priority {
	Interactive,
	Background,
}

impl CallKind {
	fn priority(&self) -> Priority {
		match self {
			Self::DisplayUsers(_) => Priority::Background,
			_ => Priority::Interactive,
		}
	}

	fn is_measured(&self) -> bool {
		self.priority() == Priority::Interactive && !matches!(self, Self::Init)
	}

	fn is_gesture(&self) -> bool {
		matches!(
			self,
			Self::UiAction { .. } | Self::Command { .. } | Self::ViewEvent(_) | Self::MenuItem { .. }
		)
	}

	fn timeout(&self, timeouts: &HookTimeouts) -> Duration {
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
			Self::ViewEvent(_) | Self::MenuItem { .. } => timeouts.view_event,
			Self::RenderView { .. } => timeouts.render_view,
			Self::Intercept { .. } => timeouts.intercept_message,
			Self::Timer { .. } => timeouts.timer,
			Self::EmojiPick { .. } => timeouts.emoji_pick,
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
	closed: bool,
}

impl QueueState {
	fn forget(&mut self, call: &Call) {
		if let CallKind::Timer { id, .. } = &call.kind {
			self.timers.remove(id);
		}
	}

	fn push_event(&mut self, call: Call, cap: usize) {
		if self.events.len() >= cap
			&& let Some(oldest) = self.events.pop_front()
		{
			self.forget(&oldest);
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
			if let Some(call) = state.events.pop_front() {
				state.forget(&call);
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
		drop(state);
		self.ready.notify_all();
	}
}

struct Running {
	store: Store<HostState>,
	plugin: Plugin,
	hooks: Option<hooks::Guest>,
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

impl WorkerContext {
	fn epoch_ticks(&self, timeout: Duration) -> u64 {
		let tick = self.runtime.config.epoch_tick;
		timeout.div_duration_f64(tick).ceil() as u64 + 1
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
		let state = HostState::new(Arc::clone(&self.id), Arc::clone(&self.runtime.services), config)
			.with_health(Arc::clone(&self.health));
		let mut store = Store::new(&self.runtime.engine, state);
		store.limiter(|state| &mut state.limits);
		store.epoch_deadline_trap();
		store.set_epoch_deadline(self.epoch_ticks(config.timeouts.init));
		let loaded = self.pre.instance.instantiate(&mut store).and_then(|instance| {
			let plugin = self.pre.guest.load(&mut store, &instance)?;
			let hooks = match &self.pre.hooks {
				Some(indices) => Some(indices.load(&mut store, &instance)?),
				None => None,
			};
			Ok((plugin, hooks))
		});
		let (plugin, hooks) = match loaded {
			Ok(loaded) => loaded,
			Err(error) => return Err(self.classify(&error, store.data().limits.memory_exceeded)),
		};
		match plugin.oxidcord_plugin_guest().call_init(&mut store) {
			Ok(Ok(())) => Ok(Running { store, plugin, hooks }),
			Ok(Err(message)) => Err(CallError::InitFailed(message)),
			Err(error) => Err(self.classify(&error, store.data().limits.memory_exceeded)),
		}
	}

	fn execute(&self, running: &mut Running, kind: CallKind, timeout: Duration) -> Result<Output, CallError> {
		let Running { store, plugin, hooks } = running;
		store.set_epoch_deadline(self.epoch_ticks(timeout));
		let guest = plugin.oxidcord_plugin_guest();
		let gesture = kind.is_gesture();
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
			kind => execute_hook(store, hooks.as_ref(), kind),
		};
		if gesture {
			store.data_mut().end_gesture();
		}
		result.map_err(|error| self.classify(&error, store.data().limits.memory_exceeded))
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
		(Some(hooks), CallKind::ViewEvent(event)) => {
			hooks.call_on_view_event(&mut *store, &event).map(Output::Reply)
		}
		(None, CallKind::ViewEvent(_)) => Ok(Output::Reply(quiet_reply())),
		(Some(hooks), CallKind::MenuItem { menu, key, context }) => hooks
			.call_on_menu_item(&mut *store, &menu, &key, &context)
			.map(|()| Output::Done),
		(Some(hooks), CallKind::RenderView { point, context }) => hooks
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
		match receiver.recv_timeout(timeout + self.runtime.config.caller_grace) {
			Ok(result) => result,
			Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
				cancelled.store(true, Ordering::Release);
				if started.load(Ordering::Acquire) {
					self.abandon(&queue);
				}
				Err(CallError::Timeout)
			}
			Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
				Err(if self.health.state() == PluginState::Disabled {
					CallError::Disabled
				} else {
					CallError::Stopped
				})
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
	pub fn view_event(&self, event: ViewEvent) -> Result<ViewReply, CallError> {
		match self.call(CallKind::ViewEvent(event))? {
			Output::Reply(reply) => Ok(reply),
			_ => Err(mismatch()),
		}
	}

	pub fn menu_item(&self, menu: String, key: String, context: UiContext) -> Result<(), CallError> {
		self.call(CallKind::MenuItem { menu, key, context }).map(|_| ())
	}

	pub fn render_view(&self, point: String, context: UiContext) -> Result<Option<ViewTree>, CallError> {
		match self.call(CallKind::RenderView { point, context })? {
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
