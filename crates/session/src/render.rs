use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Condvar, Mutex};
use plugin_host::PluginManager;
use plugin_host::api::model::Lifecycle;
use plugin_host::convert::{DELETED_FLAG, clip, message_full};
use protocol::{Message, Snowflake};
use state::RenderedMessage;
use tokio::sync::mpsc::UnboundedSender;

use crate::broker::view_of;
use crate::task::Internal;

const FLUSH_INTERVAL: Duration = Duration::from_millis(30);
const SAMPLE_CAPACITY: usize = 512;
const P95: f64 = 0.95;
const HISTORY_CAPACITY: usize = 2_000;
const HISTORY_VERSIONS: usize = 10;
const HISTORY_CHARS: usize = 2_000;

pub(crate) struct RenderJob {
	pub(crate) message: Arc<Message>,
	pub(crate) guild_id: Option<Snowflake>,
	pub(crate) epoch: u64,
	pub(crate) deleted: bool,
	pub(crate) mine: bool,
	pub(crate) intercept: bool,
}

pub(crate) struct RenderResult {
	pub(crate) message: Arc<Message>,
	pub(crate) rendered: Arc<RenderedMessage>,
}

#[derive(Default)]
pub(crate) struct RenderStats {
	samples: Mutex<VecDeque<f32>>,
}

impl RenderStats {
	fn record(&self, elapsed: Duration) {
		let mut samples = self.samples.lock();
		if samples.len() == SAMPLE_CAPACITY {
			samples.pop_front();
		}
		samples.push_back(elapsed.as_secs_f32() * 1000.0);
	}

	pub(crate) fn p95_ms(&self) -> Option<f64> {
		let mut sorted: Vec<f32> = self.samples.lock().iter().copied().collect();
		if sorted.is_empty() {
			return None;
		}
		sorted.sort_by(f32::total_cmp);
		let index = ((sorted.len() as f64 * P95).ceil() as usize).clamp(1, sorted.len()) - 1;
		Some(f64::from(sorted[index]))
	}
}

struct Queued {
	sequence: u64,
	job: RenderJob,
}

#[derive(Default)]
struct Queue {
	jobs: Vec<Queued>,
	next_sequence: u64,
	closed: bool,
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
		stats: Arc<RenderStats>,
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
			queue
				.jobs
				.retain(|queued| queued.job.message.id != job.message.id);
			queue.next_sequence += 1;
			let sequence = queue.next_sequence;
			queue.jobs.push(Queued { sequence, job });
		}
		drop(queue);
		self.shared.ready.notify_one();
	}

	pub(crate) fn select(&self, channel_id: Snowflake) {
		self.shared.selected.store(channel_id.get(), Ordering::Release);
	}

	pub(crate) fn set_epoch(&self, epoch: u64) {
		self.shared.epoch.store(epoch, Ordering::Release);
		self.shared.queue.lock().jobs.clear();
	}
}

impl Drop for RenderWorker {
	fn drop(&mut self) {
		self.shared.queue.lock().closed = true;
		self.shared.ready.notify_all();
	}
}

fn next_job(shared: &Shared) -> Option<(RenderJob, bool)> {
	let mut queue = shared.queue.lock();
	loop {
		if queue.closed {
			return None;
		}
		let selected = shared.selected.load(Ordering::Acquire);
		let best = queue
			.jobs
			.iter()
			.enumerate()
			.max_by_key(|(_, queued)| (queued.job.message.channel_id.get() == selected, queued.sequence))
			.map(|(index, _)| index);
		if let Some(index) = best {
			let queued = queue.jobs.swap_remove(index);
			return Some((queued.job, queue.jobs.is_empty()));
		}
		shared.ready.wait(&mut queue);
	}
}

struct Seen {
	content: String,
	previous: Vec<String>,
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
		let content = clip(&message.content, HISTORY_CHARS);
		if !self.seen.contains_key(&message.id) {
			self.order.push_back(message.id);
			if self.order.len() > HISTORY_CAPACITY
				&& let Some(oldest) = self.order.pop_front()
			{
				self.seen.remove(&oldest);
			}
		}
		let seen = self.seen.entry(message.id).or_insert_with(|| Seen {
			content: content.clone(),
			previous: Vec::new(),
			deleted_at: None,
		});
		if seen.content != content {
			let earlier = std::mem::replace(&mut seen.content, content);
			seen.previous.push(earlier);
			if seen.previous.len() > HISTORY_VERSIONS {
				seen.previous.remove(0);
			}
		}
		if deleted {
			return Lifecycle::Deleted(*seen.deleted_at.get_or_insert_with(now_ms));
		}
		if message.edited_timestamp.is_some() {
			Lifecycle::Edited(seen.previous.clone())
		} else {
			Lifecycle::Live
		}
	}
}

fn render(manager: &PluginManager, job: &RenderJob, history: &mut History) -> RenderedMessage {
	let message = &job.message;
	let mut view = view_of(message, || job.guild_id);
	if job.deleted {
		view.flags |= DELETED_FLAG;
	}
	let mut hidden = !manager.filter_message(&view);
	let mut ast = markdown::parse(&message.content).nodes;
	let mut patches = Vec::new();
	if !hidden {
		let mut kept = !job.deleted;
		if job.deleted {
			if let Some(rendered) = manager.render_deleted(&view, &ast) {
				ast = rendered;
				kept = true;
			}
		} else {
			ast = manager.render_message(&view, ast);
		}
		if job.intercept {
			let lifecycle = history.observe(message, job.deleted);
			let outcome = manager.intercept(message_full(message, view, job.mine, lifecycle), ast);
			ast = outcome.ast;
			kept |= outcome.retain;
			patches = outcome.patches;
		}
		hidden = !kept;
	}
	manager.store_patches(message.id.get(), patches);
	RenderedMessage {
		ast,
		hidden,
		deleted: job.deleted,
		revision: job.epoch,
	}
}

fn worker_main(
	shared: &Shared,
	manager: &PluginManager,
	stats: &RenderStats,
	results: &UnboundedSender<Internal>,
) {
	let mut finished = Vec::new();
	let mut history = History::default();
	let mut last_flush = Instant::now();
	while let Some((job, drained)) = next_job(shared) {
		if job.epoch == shared.epoch.load(Ordering::Acquire) {
			let started = Instant::now();
			let rendered = Arc::new(render(manager, &job, &mut history));
			stats.record(started.elapsed());
			finished.push(RenderResult {
				message: job.message,
				rendered,
			});
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
