use std::mem;
use std::sync::Arc;

use anyhow::Result;
use protocol::{Message, Ready, Snowflake};
use serde::de::DeserializeOwned;
use store::{ReadState, Store, Table};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const QUEUE_CAPACITY: usize = 1024;
const BATCH_LIMIT: usize = 256;
const READY_KEY: &str = "session.ready";
pub(crate) const CACHED_MESSAGES: u32 = 50;
pub(crate) const RECENT_EMOJI_KEY: &str = "expressions.recent.emoji";
pub(crate) const RECENT_GIFS_KEY: &str = "expressions.recent.gifs";
pub(crate) const RECENT_STICKERS_KEY: &str = "expressions.recent.stickers";

pub(crate) enum Job {
	Ready(Box<Ready>),
	Messages {
		channel_id: Snowflake,
		messages: Vec<Arc<Message>>,
	},
	DeleteMessages(Vec<Snowflake>),
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

pub(crate) struct Persistor {
	queue: mpsc::Sender<Job>,
	task: JoinHandle<()>,
}

impl Persistor {
	pub(crate) fn spawn(store: Arc<Store>, runtime: &Handle) -> Self {
		let (queue, jobs) = mpsc::channel(QUEUE_CAPACITY);
		let task = runtime.spawn(write_loop(store, jobs));
		Self { queue, task }
	}

	pub(crate) fn submit(&self, job: Job) {
		if self.queue.try_send(job).is_err() {
			tracing::warn!("cache write queue is full, dropping a write");
		}
	}

	pub(crate) async fn close(self) {
		drop(self.queue);
		let _ = self.task.await;
	}
}

async fn write_loop(store: Arc<Store>, mut jobs: mpsc::Receiver<Job>) {
	let mut batch = Vec::new();
	while jobs.recv_many(&mut batch, BATCH_LIMIT).await > 0 {
		let pending = mem::take(&mut batch);
		let store = store.clone();
		let written = tokio::task::spawn_blocking(move || {
			for job in pending {
				if let Err(error) = write(&store, job) {
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

fn write(store: &Store, job: Job) -> Result<()> {
	match job {
		Job::Ready(ready) => write_ready(store, &ready),
		Job::Messages { channel_id, messages } => {
			let encoded = messages
				.iter()
				.map(|message| Ok((message.id.get(), serde_json::to_vec(&**message)?)))
				.collect::<Result<Vec<_>>>()?;
			let rows: Vec<(u64, &[u8])> = encoded.iter().map(|(id, json)| (*id, json.as_slice())).collect();
			Ok(store.db.put_messages_batch(channel_id.get(), &rows)?)
		}
		Job::DeleteMessages(ids) => {
			for id in ids {
				store.db.delete_message(id.get())?;
			}
			Ok(())
		}
		Job::ReadState(state) => Ok(store.db.set_read_state(&state)?),
		Job::Recent { key, json } => Ok(store.db.kv_set(key, &json)?),
		Job::Draft { channel_id, content } => {
			if content.is_empty() {
				Ok(store.db.clear_draft(channel_id.get())?)
			} else {
				Ok(store.db.set_draft(channel_id.get(), &content)?)
			}
		}
	}
}

fn write_ready(store: &Store, ready: &Ready) -> Result<()> {
	let db = &store.db;
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
		db.put_json(
			Table::Channels,
			channel.id.get(),
			None,
			&serde_json::to_vec(channel)?,
		)?;
	}
	for user in ready.users.iter() {
		db.put_json(Table::Users, user.id.get(), None, &serde_json::to_vec(user)?)?;
	}
	for read_state in ready.read_state.entries.iter() {
		db.set_read_state(&stored_read_state(read_state))?;
	}
	db.kv_set(READY_KEY, &serde_json::to_vec(ready)?)?;
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

pub(crate) fn clear_cache(store: &Store) {
	if let Err(error) = store.db.kv_set(READY_KEY, &[]) {
		tracing::warn!(%error, "could not clear the cached session snapshot");
	}
}

pub(crate) fn load_ready(store: &Store) -> Option<Ready> {
	let bytes = store
		.db
		.kv_get(READY_KEY)
		.ok()
		.flatten()
		.filter(|bytes| !bytes.is_empty())?;
	match serde_json::from_slice::<Ready>(&bytes) {
		Ok(mut ready) => {
			ready.normalize_guilds();
			Some(ready)
		}
		Err(error) => {
			tracing::warn!(%error, "cached session snapshot is unreadable");
			None
		}
	}
}

pub(crate) fn load_read_states(store: &Store) -> Vec<ReadState> {
	store.db.all_read_state().unwrap_or_else(|error| {
		tracing::warn!(%error, "could not read cached read state");
		Vec::new()
	})
}

pub(crate) fn load_messages(store: &Store, channel_id: Snowflake) -> Vec<Message> {
	let rows = store
		.db
		.recent_messages(channel_id.get(), CACHED_MESSAGES)
		.unwrap_or_else(|error| {
			tracing::warn!(%error, "could not read cached messages");
			Vec::new()
		});
	rows.iter()
		.filter_map(|(_, json)| serde_json::from_slice(json).ok())
		.collect()
}

pub(crate) fn load_draft(store: &Store, channel_id: Snowflake) -> Option<String> {
	store.db.get_draft(channel_id.get()).ok().flatten()
}

pub(crate) fn load_recent<T: DeserializeOwned>(store: &Store, key: &str) -> Vec<T> {
	store
		.db
		.kv_get(key)
		.ok()
		.flatten()
		.and_then(|bytes| serde_json::from_slice(&bytes).ok())
		.unwrap_or_default()
}
