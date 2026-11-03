mod activity;
mod frame;
mod ipc;
mod link;
mod ws;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use parking_lot::{Mutex, RwLock};
use protocol::{Snowflake, User};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::apps::Apps;
use crate::engine::{ActivityRequest, Input, RpcEndpoints};

pub use ipc::socket_name as ipc_socket_name;

pub(crate) const MAX_LINKS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RpcEvent {
	Join,
	Spectate,
	JoinRequest,
}

impl RpcEvent {
	pub fn parse(name: &str) -> Option<Self> {
		match name {
			"ACTIVITY_JOIN" => Some(Self::Join),
			"ACTIVITY_SPECTATE" => Some(Self::Spectate),
			"ACTIVITY_JOIN_REQUEST" => Some(Self::JoinRequest),
			_ => None,
		}
	}

	pub fn name(self) -> &'static str {
		match self {
			Self::Join => "ACTIVITY_JOIN",
			Self::Spectate => "ACTIVITY_SPECTATE",
			Self::JoinRequest => "ACTIVITY_JOIN_REQUEST",
		}
	}

	fn bit(self) -> u8 {
		1 << self as u8
	}
}

struct Subscriber {
	application_id: u64,
	party_id: Option<String>,
	events: u8,
	sender: mpsc::UnboundedSender<Value>,
}

pub(crate) struct RpcShared {
	pub apps: Apps,
	pub inputs: mpsc::UnboundedSender<Input>,
	pub requests: mpsc::UnboundedSender<ActivityRequest>,
	user: RwLock<Option<User>>,
	subscribers: Mutex<HashMap<u64, Subscriber>>,
	pending: Mutex<HashSet<(u64, Snowflake)>>,
	next_link: AtomicU64,
	live: AtomicUsize,
}

pub(crate) fn user_json(user: &User) -> Value {
	json!({
		"id": user.id,
		"username": user.username,
		"discriminator": user.discriminator.as_deref().unwrap_or("0"),
		"global_name": user.global_name,
		"avatar": user.avatar,
		"avatar_decoration_data": user.avatar_decoration_data,
		"bot": user.bot.unwrap_or(false),
		"flags": user.public_flags.bits(),
		"premium_type": user.premium_type.map_or(0, i64::from),
	})
}

impl RpcShared {
	pub fn new(
		apps: Apps,
		inputs: mpsc::UnboundedSender<Input>,
		requests: mpsc::UnboundedSender<ActivityRequest>,
	) -> Self {
		Self {
			apps,
			inputs,
			requests,
			user: RwLock::new(None),
			subscribers: Mutex::new(HashMap::new()),
			pending: Mutex::new(HashSet::new()),
			next_link: AtomicU64::new(1),
			live: AtomicUsize::new(0),
		}
	}

	pub fn register(&self, link: u64, application_id: u64, sender: mpsc::UnboundedSender<Value>) {
		self.subscribers.lock().insert(
			link,
			Subscriber {
				application_id,
				party_id: None,
				events: 0,
				sender,
			},
		);
	}

	pub fn forget(&self, link: u64) {
		self.subscribers.lock().remove(&link);
	}

	pub fn set_party(&self, link: u64, party_id: Option<String>) {
		if let Some(subscriber) = self.subscribers.lock().get_mut(&link) {
			subscriber.party_id = party_id;
		}
	}

	pub fn subscribe(&self, link: u64, event: RpcEvent) {
		if let Some(subscriber) = self.subscribers.lock().get_mut(&link) {
			subscriber.events |= event.bit();
		}
	}

	pub fn unsubscribe(&self, link: u64, event: RpcEvent) {
		if let Some(subscriber) = self.subscribers.lock().get_mut(&link) {
			subscriber.events &= !event.bit();
		}
	}

	pub fn dispatch(
		&self,
		application_id: u64,
		event: RpcEvent,
		party_id: Option<&str>,
		data: &Value,
	) -> bool {
		let payload = json!({ "cmd": "DISPATCH", "data": data, "evt": event.name(), "nonce": Value::Null });
		let mut delivered = false;
		for subscriber in self.subscribers.lock().values() {
			if subscriber.application_id == application_id
				&& subscriber.events & event.bit() != 0
				&& party_id.is_none_or(|party_id| subscriber.party_id.as_deref() == Some(party_id))
				&& subscriber.sender.send(payload.clone()).is_ok()
			{
				delivered = true;
			}
		}
		delivered
	}

	pub fn add_pending(&self, application_id: u64, user_id: Snowflake) -> bool {
		self.pending.lock().insert((application_id, user_id))
	}

	pub fn take_pending(&self, application_id: u64, user_id: Snowflake) -> bool {
		self.pending.lock().remove(&(application_id, user_id))
	}

	pub fn set_user(&self, user: Option<User>) {
		*self.user.write() = user;
	}

	pub fn user_json(&self) -> Value {
		self.user.read().as_ref().map_or(Value::Null, user_json)
	}

	pub fn next_link(&self) -> u64 {
		self.next_link.fetch_add(1, Ordering::Relaxed)
	}

	pub fn admit(&self) -> bool {
		let mut live = self.live.load(Ordering::Relaxed);
		loop {
			if live >= MAX_LINKS {
				return false;
			}
			match self
				.live
				.compare_exchange_weak(live, live + 1, Ordering::Relaxed, Ordering::Relaxed)
			{
				Ok(_) => return true,
				Err(current) => live = current,
			}
		}
	}

	pub fn release(&self) {
		self.live.fetch_sub(1, Ordering::Relaxed);
	}
}

pub(crate) async fn serve(
	shared: Arc<RpcShared>,
	mut enabled: watch::Receiver<bool>,
	endpoints: watch::Sender<RpcEndpoints>,
) {
	loop {
		if !*enabled.borrow() {
			if enabled.changed().await.is_err() {
				return;
			}
			continue;
		}
		let ipc = ipc::bind();
		let ws = ws::bind().await;
		endpoints.send_replace(RpcEndpoints {
			ipc: ipc.as_ref().map(|(index, _)| *index),
			port: ws.as_ref().map(|(port, _)| *port),
		});
		let tasks: Vec<JoinHandle<()>> = ipc
			.map(|(_, listener)| tokio::spawn(ipc::accept_loop(shared.clone(), listener, enabled.clone())))
			.into_iter()
			.chain(ws.map(|(_, listener)| {
				tokio::spawn(ws::accept_loop(shared.clone(), listener, enabled.clone()))
			}))
			.collect();
		let closed = loop {
			if enabled.changed().await.is_err() {
				break true;
			}
			if !*enabled.borrow() {
				break false;
			}
		};
		for task in &tasks {
			task.abort();
		}
		endpoints.send_replace(RpcEndpoints::default());
		if closed {
			return;
		}
	}
}
