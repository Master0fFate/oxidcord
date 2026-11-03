use std::mem;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use protocol::{Channel, LenientVec, Message, Ready, Snowflake};
use serde::de::DeserializeOwned;
use store::{Database, ReadState, Table};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const QUEUE_CAPACITY: usize = 1024;
const BATCH_LIMIT: usize = 256;
const READY_KEY: &str = "session.ready";
const LAZY_CHANNELS_KEY: &str = "ready.lazy_private_channels";
pub(crate) const CACHED_MESSAGES: u32 = 50;
const KEPT_MESSAGES_PER_CHANNEL: u32 = 200;
pub(crate) const RECENT_EMOJI_KEY: &str = "expressions.recent.emoji";
pub(crate) const LAST_SELECTION_KEY: &str = "navigation.last_selection";
pub(crate) const RECENT_GIFS_KEY: &str = "expressions.recent.gifs";
pub(crate) const RECENT_STICKERS_KEY: &str = "expressions.recent.stickers";

pub(crate) enum Job {
	Ready(Box<Ready>),
	PrivateChannels(Vec<Channel>),
	Messages {
		channel_id: Snowflake,
		messages: Vec<Arc<Message>>,
	},
	DeleteMessages(Vec<Snowflake>),
	DropChannels(Vec<Snowflake>),
	Wipe,
	ReadState(ReadState),
	Draft {
		channel_id: Snowflake,
		content: String,
	},
	Recent {
		key: &'static str,
		json: Vec<u8>,
	},
}

struct Queued {
	db: Arc<Database>,
	job: Job,
}

struct AccountCache {
	user_id: Snowflake,
	db: Arc<Database>,
}

struct Opening {
	user_id: Snowflake,
	held: Vec<Job>,
}

pub(crate) struct Persistor {
	runtime: Handle,
	queue: mpsc::Sender<Queued>,
	task: JoinHandle<()>,
	cache: Option<AccountCache>,
	opening: Mutex<Option<Opening>>,
}

impl Persistor {
	pub(crate) fn spawn(runtime: &Handle) -> Self {
		let (queue, jobs) = mpsc::channel(QUEUE_CAPACITY);
		let task = runtime.spawn(write_loop(jobs));
		Self {
			runtime: runtime.clone(),
			queue,
			task,
			cache: None,
			opening: Mutex::new(None),
		}
	}

	pub(crate) fn begin_open(&mut self, user_id: Snowflake) {
		self.cache = None;
		*self.opening.get_mut() = Some(Opening {
			user_id,
			held: Vec::new(),
		});
	}

	pub(crate) fn is_opening(&self, user_id: Snowflake) -> bool {
		self.opening
			.lock()
			.as_ref()
			.is_some_and(|opening| opening.user_id == user_id)
	}

	pub(crate) fn attach(&mut self, user_id: Snowflake, db: Arc<Database>) {
		let held = match self.opening.get_mut().take() {
			Some(opening) if opening.user_id == user_id => opening.held,
			_ => Vec::new(),
		};
		self.cache = Some(AccountCache { user_id, db });
		for job in held {
			self.submit(job);
		}
	}

	pub(crate) fn detach(&mut self) {
		self.cache = None;
		*self.opening.get_mut() = None;
	}

	pub(crate) fn owner(&self) -> Option<Snowflake> {
		self.cache.as_ref().map(|cache| cache.user_id)
	}

	pub(crate) fn database(&self) -> Option<Arc<Database>> {
		self.cache.as_ref().map(|cache| Arc::clone(&cache.db))
	}

	pub(crate) fn submit(&self, job: Job) {
		let Some(cache) = &self.cache else {
			if let Some(opening) = self.opening.lock().as_mut()
				&& opening.held.len() < QUEUE_CAPACITY
			{
				opening.held.push(job);
			}
			return;
		};
		let queued = Queued {
			db: Arc::clone(&cache.db),
			job,
		};
		if self.queue.try_send(queued).is_err() {
			tracing::warn!("cache write queue is full, dropping a write");
		}
	}

	pub(crate) fn wipe(&mut self) {
		let cache = self.cache.take();
		*self.opening.get_mut() = None;
		let Some(AccountCache { db, .. }) = cache else {
			return;
		};
		let queue = self.queue.clone();
		self.runtime.spawn(async move {
			if queue.send(Queued { db, job: Job::Wipe }).await.is_err() {
				tracing::warn!("cache wipe was not queued");
			}
		});
	}

	pub(crate) async fn close(self) {
		drop(self.queue);
		let _ = self.task.await;
	}
}

async fn write_loop(mut jobs: mpsc::Receiver<Queued>) {
	let mut batch = Vec::new();
	while jobs.recv_many(&mut batch, BATCH_LIMIT).await > 0 {
		let pending = mem::take(&mut batch);
		let written = tokio::task::spawn_blocking(move || {
			for Queued { db, job } in pending {
				if let Err(error) = write(&db, job) {
					tracing::warn!(%error, "cache write failed");
				}
			}
		})
		.await;
		if written.is_err() {
			tracing::warn!("cache writer panicked");
		}
	}
}

fn write(db: &Database, job: Job) -> Result<()> {
	match job {
		Job::Ready(mut ready) => {
			ready.align_merged_members();
			write_ready(db, &ready)
		}
		Job::PrivateChannels(channels) => write_private_channels(db, channels),
		Job::Messages { channel_id, messages } => {
			let floor = db.keep_floor(channel_id.get(), KEPT_MESSAGES_PER_CHANNEL)?;
			let encoded = messages
				.iter()
				.filter(|message| floor.is_none_or(|floor| message.id.get() >= floor))
				.map(|message| Ok((message.id.get(), serde_json::to_vec(&**message)?)))
				.collect::<Result<Vec<_>>>()?;
			if encoded.is_empty() {
				return Ok(());
			}
			let rows: Vec<(u64, &[u8])> = encoded.iter().map(|(id, json)| (*id, json.as_slice())).collect();
			db.put_messages_batch(channel_id.get(), &rows)?;
			Ok(db.trim_channel(channel_id.get(), KEPT_MESSAGES_PER_CHANNEL)?)
		}
		Job::DeleteMessages(ids) => {
			for id in ids {
				db.delete_message(id.get())?;
			}
			Ok(())
		}
		Job::DropChannels(ids) => {
			let ids: Vec<u64> = ids.into_iter().map(Snowflake::get).collect();
			Ok(db.drop_channels(&ids)?)
		}
		Job::Wipe => Ok(db.wipe()?),
		Job::ReadState(state) => Ok(db.set_read_state(&state)?),
		Job::Recent { key, json } => Ok(db.kv_set(key, &json)?),
		Job::Draft { channel_id, content } => {
			if content.is_empty() {
				Ok(db.clear_draft(channel_id.get())?)
			} else {
				Ok(db.set_draft(channel_id.get(), &content)?)
			}
		}
	}
}

fn write_ready(db: &Database, ready: &Ready) -> Result<()> {
	for guild in ready.guilds.iter() {
		db.put_json(Table::Guilds, guild.id.get(), None, &serde_json::to_vec(guild)?)?;
		for channel in guild.channels.iter().chain(guild.threads.iter()) {
			db.put_json(
				Table::Channels,
				channel.id.get(),
				Some(guild.id.get()),
				&serde_json::to_vec(channel)?,
			)?;
		}
	}
	for channel in ready.private_channels.iter() {
		put_private_channel(db, channel)?;
	}
	for user in ready.users.iter() {
		db.put_json(Table::Users, user.id.get(), None, &serde_json::to_vec(user)?)?;
	}
	for read_state in ready.read_state.entries.iter() {
		db.set_read_state(&stored_read_state(read_state))?;
	}
	db.kv_set(READY_KEY, &serde_json::to_vec(ready)?)?;
	db.kv_delete(LAZY_CHANNELS_KEY)?;
	Ok(())
}

fn put_private_channel(db: &Database, channel: &Channel) -> Result<()> {
	Ok(db.put_json(
		Table::Channels,
		channel.id.get(),
		None,
		&serde_json::to_vec(channel)?,
	)?)
}

fn write_private_channels(db: &Database, channels: Vec<Channel>) -> Result<()> {
	let mut lazy = load_lazy_channels(db);
	for channel in channels {
		put_private_channel(db, &channel)?;
		if !lazy.iter().any(|known| known.id == channel.id) {
			lazy.push(channel);
		}
	}
	db.kv_set(LAZY_CHANNELS_KEY, &serde_json::to_vec(&lazy)?)?;
	Ok(())
}

pub(crate) fn stored_read_state(read_state: &protocol::ReadState) -> ReadState {
	ReadState {
		channel_id: read_state.id.get(),
		last_message_id: read_state.last_message_id.map(Snowflake::get),
		mention_count: read_state
			.mention_count
			.map_or(0, |count| u32::try_from(count).unwrap_or(0)),
		last_viewed: read_state.last_viewed,
	}
}

fn load_lazy_channels(db: &Database) -> Vec<Channel> {
	db.kv_get(LAZY_CHANNELS_KEY)
		.ok()
		.flatten()
		.and_then(|bytes| serde_json::from_slice::<LenientVec<Channel>>(&bytes).ok())
		.map(LenientVec::into_vec)
		.unwrap_or_default()
}

pub(crate) fn load_ready(db: &Database) -> Option<Ready> {
	let bytes = db
		.kv_get(READY_KEY)
		.ok()
		.flatten()
		.filter(|bytes| !bytes.is_empty())?;
	match serde_json::from_slice::<Ready>(&bytes) {
		Ok(mut ready) => {
			ready.normalize_guilds();
			for channel in load_lazy_channels(db) {
				if !ready.private_channels.iter().any(|known| known.id == channel.id) {
					ready.private_channels.push(channel);
				}
			}
			Some(ready)
		}
		Err(error) => {
			tracing::warn!(%error, "cached session snapshot is unreadable");
			None
		}
	}
}

pub(crate) fn load_read_states(db: &Database) -> Vec<ReadState> {
	db.all_read_state().unwrap_or_else(|error| {
		tracing::warn!(%error, "could not read cached read state");
		Vec::new()
	})
}

pub(crate) fn load_messages(db: &Database, channel_id: Snowflake) -> Vec<Message> {
	let rows = db
		.recent_messages(channel_id.get(), CACHED_MESSAGES)
		.unwrap_or_else(|error| {
			tracing::warn!(%error, "could not read cached messages");
			Vec::new()
		});
	rows.iter()
		.filter_map(|(_, json)| serde_json::from_slice(json).ok())
		.collect()
}

pub(crate) fn load_draft(db: &Database, channel_id: Snowflake) -> Option<String> {
	db.get_draft(channel_id.get()).ok().flatten()
}

pub(crate) fn load_value<T: DeserializeOwned>(db: &Database, key: &str) -> Option<T> {
	let bytes = db.kv_get(key).ok().flatten()?;
	serde_json::from_slice(&bytes).ok()
}

pub(crate) fn load_recent<T: DeserializeOwned>(db: &Database, key: &str) -> Vec<T> {
	db.kv_get(key)
		.ok()
		.flatten()
		.and_then(|bytes| serde_json::from_slice::<LenientVec<T>>(&bytes).ok())
		.map(LenientVec::into_vec)
		.unwrap_or_default()
}
