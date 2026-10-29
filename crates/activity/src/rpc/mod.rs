mod activity;
mod frame;
mod ipc;
mod link;
mod ws;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use parking_lot::RwLock;
use protocol::User;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::apps::Apps;
use crate::engine::{Input, RpcEndpoints};

pub use ipc::socket_name as ipc_socket_name;

pub(crate) const MAX_LINKS: usize = 64;

pub(crate) struct RpcShared {
	pub apps: Apps,
	pub inputs: mpsc::UnboundedSender<Input>,
	user: RwLock<Option<User>>,
	next_link: AtomicU64,
	live: AtomicUsize,
}

impl RpcShared {
	pub fn new(apps: Apps, inputs: mpsc::UnboundedSender<Input>) -> Self {
		Self {
			apps,
			inputs,
			user: RwLock::new(None),
			next_link: AtomicU64::new(1),
			live: AtomicUsize::new(0),
		}
	}

	pub fn set_user(&self, user: Option<User>) {
		*self.user.write() = user;
	}

	pub fn user_json(&self) -> Value {
		match self.user.read().as_ref() {
			Some(user) => json!({
				"id": user.id,
				"username": user.username,
				"discriminator": user.discriminator.as_deref().unwrap_or("0"),
				"global_name": user.global_name,
				"avatar": user.avatar,
				"avatar_decoration_data": user.avatar_decoration_data,
				"bot": user.bot.unwrap_or(false),
				"flags": user.public_flags.bits(),
				"premium_type": user.premium_type.map_or(0, i64::from),
			}),
			None => Value::Null,
		}
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
