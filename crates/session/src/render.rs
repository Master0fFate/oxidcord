use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Condvar, Mutex};
use plugin_host::api::model::{Lifecycle, MessageMeta};
use plugin_host::convert::{DELETED_FLAG, clip, message_full};
use plugin_host::{InterceptInput, LatencyStats, PluginManager};
use protocol::{Message, Snowflake};
use state::RenderedMessage;
use tokio::sync::mpsc::UnboundedSender;

use crate::broker::view_of;
use crate::task::Internal;

const FLUSH_INTERVAL: Duration = Duration::from_millis(30);
const HISTORY_CAPACITY: usize = 2_000;
const HISTORY_VERSIONS: usize = 10;
const HISTORY_CHARS: usize = 2_000;
const EXT_BATCH: usize = 16;

pub(crate) struct RenderJob {
	pub(crate) message: Arc<Message>,
	pub(crate) guild_id: Option<Snowflake>,
	pub(crate) epoch: u64,
	pub(crate) deleted: bool,
	pub(crate) mine: bool,
	pub(crate) intercept: bool,
	pub(crate) meta: Option<MessageMeta>,
}

pub(crate) struct RenderResult {
	pub(crate) message: Arc<Message>,
	pub(crate) rendered: Arc<RenderedMessage>,
}

#[derive(Default)]
struct Queue {
	by_sequence: BTreeMap<u64, RenderJob>,
	by_message: HashMap<Snowflake, u64>,
	by_channel: BTreeSet<(u64, u64)>,
	next_sequence: u64,
	closed: bool,
}

impl Queue {
	fn insert(&mut self, job: RenderJob) {
		self.remove_message(job.message.id);
		self.next_sequence += 1;
		let sequence = self.next_sequence;
		self.by_message.insert(job.message.id, sequence);
		self.by_channel.insert((job.message.channel_id.get(), sequence));
		self.by_sequence.insert(sequence, job);
	}

	fn remove_message(&mut self, message_id: Snowflake) {
		if let Some(sequence) = self.by_message.remove(&message_id)
			&& let Some(job) = self.by_sequence.remove(&sequence)
		{
			self.by_channel.remove(&(job.message.channel_id.get(), sequence));
		}
	}

	fn take_best(&mut self, selected: u64) -> Option<RenderJob> {
		let sequence = self
			.by_channel
			.range((selected, 0)..=(selected, u64::MAX))
			.next_back()
			.map(|&(_, sequence)| sequence)
			.or_else(|| self.by_sequence.last_key_value().map(|(&sequence, _)| sequence))?;
		let job = self.by_sequence.remove(&sequence)?;
		self.by_message.remove(&job.message.id);
		self.by_channel.remove(&(job.message.channel_id.get(), sequence));
		Some(job)
	}

	fn clear(&mut self) {
		self.by_sequence.clear();
		self.by_message.clear();
		self.by_channel.clear();
	}
}

struct Shared {
	queue: Mutex<Queue>,
	ready: Condvar,
	selected: AtomicU64,
	epoch: AtomicU64,
}

pub(crate) struct RenderWorker {
	shared: Arc<Shared>,
}

impl RenderWorker {
	pub(crate) fn spawn(
		manager: Arc<PluginManager>,
		stats: Arc<LatencyStats>,
		results: UnboundedSender<Internal>,
		epoch: u64,
	) -> Self {
		let shared = Arc::new(Shared {
			queue: Mutex::new(Queue::default()),
			ready: Condvar::new(),
			selected: AtomicU64::new(0),
			epoch: AtomicU64::new(epoch),
		});
		let worker = Arc::clone(&shared);
		let spawned = thread::Builder::new()
			.name("plugin-render".to_owned())
			.spawn(move || worker_main(&worker, &manager, &stats, &results));
		if let Err(error) = spawned {
			tracing::error!(%error, "plugin render worker did not start");
		}
		Self { shared }
	}

	pub(crate) fn submit(&self, jobs: Vec<RenderJob>) {
		let mut queue = self.shared.queue.lock();
		for job in jobs {
			queue.insert(job);
		}
		drop(queue);
		self.shared.ready.notify_one();
	}

	pub(crate) fn select(&self, channel_id: Snowflake) {
		self.shared.selected.store(channel_id.get(), Ordering::Release);
	}

	pub(crate) fn set_epoch(&self, epoch: u64) {
		self.shared.epoch.store(epoch, Ordering::Release);
		self.shared.queue.lock().clear();
	}
}

impl Drop for RenderWorker {
	fn drop(&mut self) {
		self.shared.queue.lock().closed = true;
		self.shared.ready.notify_all();
	}
}

fn next_batch(shared: &Shared, manager: &PluginManager) -> Option<(Vec<RenderJob>, bool)> {
	let mut queue = shared.queue.lock();
	loop {
		if queue.closed {
			return None;
		}
		let selected = shared.selected.load(Ordering::Acquire);
		if let Some(first) = queue.take_best(selected) {
			let limit = if manager.hook_flags().intercept_ext {
				EXT_BATCH
			} else {
				1
			};
			let mut jobs = vec![first];
			while jobs.len() < limit {
				match queue.take_best(selected) {
					Some(job) => jobs.push(job),
					None => break,
				}
			}
			return Some((jobs, queue.by_sequence.is_empty()));
		}
		shared.ready.wait(&mut queue);
	}
}

struct Seen {
	content: String,
	previous: VecDeque<String>,
	deleted_at: Option<i64>,
}

#[derive(Default)]
struct History {
	seen: HashMap<Snowflake, Seen>,
	order: VecDeque<Snowflake>,
}

fn now_ms() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_millis() as i64)
}

impl History {
	fn observe(&mut self, message: &Message, deleted: bool) -> Lifecycle {
		let content = if message.content.len() > HISTORY_CHARS {
			Cow::Owned(clip(&message.content, HISTORY_CHARS))
		} else {
			Cow::Borrowed(message.content.as_str())
		};
		if !self.seen.contains_key(&message.id) {
			self.order.push_back(message.id);
			if self.order.len() > HISTORY_CAPACITY
				&& let Some(oldest) = self.order.pop_front()
			{
				self.seen.remove(&oldest);
			}
		}
		let seen = self.seen.entry(message.id).or_insert_with(|| Seen {
			content: content.to_string(),
			previous: VecDeque::new(),
			deleted_at: None,
		});
		if seen.content != content {
			let earlier = std::mem::replace(&mut seen.content, content.into_owned());
			seen.previous.push_back(earlier);
			if seen.previous.len() > HISTORY_VERSIONS {
				seen.previous.pop_front();
			}
		}
		if deleted {
			return Lifecycle::Deleted(*seen.deleted_at.get_or_insert_with(now_ms));
		}
		if message.edited_timestamp.is_some() {
			Lifecycle::Edited(seen.previous.iter().cloned().collect())
		} else {
			Lifecycle::Live
		}
	}
}

struct Awaiting {
	message_id: Snowflake,
	kept: bool,
	deleted: bool,
	revision: u64,
}

enum Stage {
	Done(RenderedMessage),
	Intercept {
		awaiting: Awaiting,
		input: Box<InterceptInput>,
	},
}

fn prepare(manager: &PluginManager, job: &RenderJob, history: &mut History) -> Stage {
	let message = &job.message;
	let hooks = manager.render_hooks(job.guild_id.map(Snowflake::get));
	if !hooks.filter && !hooks.render && !job.intercept {
		manager.store_patches(message.id.get(), Vec::new());
		return Stage::Done(RenderedMessage {
			ast: markdown::parse(&message.content).nodes,
			hidden: job.deleted,
			deleted: job.deleted,
			revision: job.epoch,
		});
	}
	let mut view = view_of(message, || job.guild_id);
	if job.deleted {
		view.flags |= DELETED_FLAG;
	}
	let view = Arc::new(view);
	let mut ast = markdown::parse(&message.content).nodes;
	if !manager.filter_message(&view) {
		manager.store_patches(message.id.get(), Vec::new());
		return Stage::Done(RenderedMessage {
			ast,
			hidden: true,
			deleted: job.deleted,
			revision: job.epoch,
		});
	}
	let mut kept = !job.deleted;
	if job.deleted {
		if let Some(rendered) = manager.render_deleted(&view, &ast) {
			ast = rendered;
			kept = true;
		}
	} else {
		ast = manager.render_message(&view, ast);
	}
	if !job.intercept {
		manager.store_patches(message.id.get(), Vec::new());
		return Stage::Done(RenderedMessage {
			ast,
			hidden: !kept,
			deleted: job.deleted,
			revision: job.epoch,
		});
	}
	let lifecycle = history.observe(message, job.deleted);
	let view = Arc::try_unwrap(view).unwrap_or_else(|shared| (*shared).clone());
	Stage::Intercept {
		awaiting: Awaiting {
			message_id: message.id,
			kept,
			deleted: job.deleted,
			revision: job.epoch,
		},
		input: Box::new(InterceptInput {
			full: message_full(message, view, job.mine, lifecycle),
			ast,
			meta: job.meta,
		}),
	}
}

fn render_batch(manager: &PluginManager, jobs: &[RenderJob], history: &mut History) -> Vec<RenderedMessage> {
	let mut staged: Vec<Result<RenderedMessage, Awaiting>> = Vec::with_capacity(jobs.len());
	let mut inputs = Vec::new();
	for job in jobs {
		match prepare(manager, job, history) {
			Stage::Done(rendered) => staged.push(Ok(rendered)),
			Stage::Intercept { awaiting, input } => {
				inputs.push(*input);
				staged.push(Err(awaiting));
			}
		}
	}
	let mut outcomes = manager.intercept_many(inputs).into_iter();
	staged
		.into_iter()
		.filter_map(|entry| match entry {
			Ok(rendered) => Some(rendered),
			Err(awaiting) => {
				let outcome = outcomes.next()?;
				manager.store_patches(awaiting.message_id.get(), outcome.patches);
				Some(RenderedMessage {
					ast: outcome.ast,
					hidden: !(awaiting.kept || outcome.retain),
					deleted: awaiting.deleted,
					revision: awaiting.revision,
				})
			}
		})
		.collect()
}

fn worker_main(
	shared: &Shared,
	manager: &PluginManager,
	stats: &LatencyStats,
	results: &UnboundedSender<Internal>,
) {
	let mut finished = Vec::new();
	let mut history = History::default();
	let mut last_flush = Instant::now();
	while let Some((mut jobs, drained)) = next_batch(shared, manager) {
		let current = shared.epoch.load(Ordering::Acquire);
		jobs.retain(|job| job.epoch == current);
		if !jobs.is_empty() {
			let started = Instant::now();
			let rendered = render_batch(manager, &jobs, &mut history);
			let per_message = started.elapsed() / u32::try_from(jobs.len()).unwrap_or(1);
			for (job, rendered) in jobs.into_iter().zip(rendered) {
				stats.record(per_message);
				finished.push(RenderResult {
					message: job.message,
					rendered: Arc::new(rendered),
				});
			}
		}
		if !finished.is_empty() && (drained || last_flush.elapsed() >= FLUSH_INTERVAL) {
			if results
				.send(Internal::Rendered(std::mem::take(&mut finished)))
				.is_err()
			{
				return;
			}
			last_flush = Instant::now();
		}
	}
}
