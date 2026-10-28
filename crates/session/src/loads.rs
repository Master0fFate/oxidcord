use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use plugin_host::api::HostError;
use plugin_host::api::User;
use plugin_host::api::model::UserProfile;
use protocol::{Message, Snowflake};
use tokio::runtime::Handle;
use tokio::sync::oneshot;

pub(crate) const LOAD_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const DIALOG_TIMEOUT: Duration = Duration::from_secs(600);
const FETCHED_MESSAGES: usize = 128;

struct Flight<V> {
	result: Mutex<Option<Result<V, HostError>>>,
	done: Condvar,
}

pub(crate) struct Flights<K, V> {
	active: Mutex<HashMap<K, Arc<Flight<V>>>>,
}

impl<K, V> Default for Flights<K, V> {
	fn default() -> Self {
		Self {
			active: Mutex::new(HashMap::new()),
		}
	}
}

impl<K: Hash + Eq + Clone, V: Clone> Flights<K, V> {
	pub(crate) fn run(&self, key: K, load: impl FnOnce() -> Result<V, HostError>) -> Result<V, HostError> {
		let (flight, leads) = {
			let mut active = self.active.lock();
			match active.get(&key) {
				Some(flight) => (Arc::clone(flight), false),
				None => {
					let flight = Arc::new(Flight {
						result: Mutex::new(None),
						done: Condvar::new(),
					});
					active.insert(key.clone(), Arc::clone(&flight));
					(flight, true)
				}
			}
		};
		if leads {
			let result = load();
			*flight.result.lock() = Some(result.clone());
			flight.done.notify_all();
			self.active.lock().remove(&key);
			return result;
		}
		let deadline = Instant::now() + LOAD_TIMEOUT;
		let mut slot = flight.result.lock();
		while slot.is_none() {
			if flight.done.wait_until(&mut slot, deadline).timed_out() {
				break;
			}
		}
		slot.clone().unwrap_or(Err(HostError::Timeout))
	}
}

#[derive(Default)]
struct FetchedMessages {
	order: VecDeque<(Snowflake, Snowflake)>,
	by_key: HashMap<(Snowflake, Snowflake), Arc<Message>>,
}

#[derive(Default)]
pub(crate) struct PluginLoads {
	fetched: Mutex<FetchedMessages>,
	pub(crate) profiles: Flights<Snowflake, Option<UserProfile>>,
	pub(crate) messages: Flights<(Snowflake, Snowflake), Option<plugin_host::api::model::MessageFull>>,
	pub(crate) users: Flights<Snowflake, Option<User>>,
	pub(crate) reactions: Flights<(Snowflake, Snowflake, String), Vec<User>>,
	pub(crate) media: Flights<String, Vec<u8>>,
}

impl PluginLoads {
	pub(crate) fn fetched_message(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
	) -> Option<Arc<Message>> {
		self.fetched.lock().by_key.get(&(channel_id, message_id)).cloned()
	}

	pub(crate) fn remember_message(&self, message: Arc<Message>) {
		let key = (message.channel_id, message.id);
		let mut fetched = self.fetched.lock();
		if fetched.by_key.insert(key, message).is_none() {
			fetched.order.push_back(key);
		}
		while fetched.order.len() > FETCHED_MESSAGES {
			if let Some(oldest) = fetched.order.pop_front() {
				fetched.by_key.remove(&oldest);
			}
		}
	}
}

pub(crate) fn await_reply<T>(handle: &Handle, receiver: oneshot::Receiver<T>) -> Result<T, HostError> {
	await_reply_for(handle, receiver, LOAD_TIMEOUT)
}

pub(crate) fn await_reply_for<T>(
	handle: &Handle,
	receiver: oneshot::Receiver<T>,
	limit: Duration,
) -> Result<T, HostError> {
	handle
		.block_on(async { tokio::time::timeout(limit, receiver).await })
		.map_err(|_| HostError::Timeout)?
		.map_err(|_| HostError::Unavailable)
}
