use std::cell::OnceCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use protocol::gateway::capability::{DEBOUNCE_MESSAGE_REACTIONS, PASSIVE_GUILD_UPDATE_V2};
use rand::Rng;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

const REPLAY_CAPACITY: usize = 500;
const DETACHED_TTL: Duration = Duration::from_secs(300);

pub enum Outgoing {
	Frame(Arc<str>),
	Close(u16),
}

#[derive(Clone)]
pub struct Link {
	pub id: u64,
	pub tx: UnboundedSender<Outgoing>,
}

struct SessionRecord {
	seq: u64,
	buffer: VecDeque<(u64, Arc<str>)>,
	link: Option<Link>,
	detached_at: Option<Instant>,
	subscribed: HashSet<u64>,
	capabilities: u64,
}

impl SessionRecord {
	fn push_event(&mut self, name: &str, data: &str) {
		self.seq += 1;
		let frame: Arc<str> = format!(r#"{{"op":0,"s":{},"t":"{name}","d":{data}}}"#, self.seq).into();
		self.buffer.push_back((self.seq, frame.clone()));
		if self.buffer.len() > REPLAY_CAPACITY {
			self.buffer.pop_front();
		}
		if let Some(link) = &self.link {
			let _ = link.tx.send(Outgoing::Frame(frame));
		}
	}
}

#[derive(Default)]
pub struct Hub {
	sessions: Mutex<HashMap<String, SessionRecord>>,
	next_link: AtomicU64,
}

impl Hub {
	pub fn next_link_id(&self) -> u64 {
		self.next_link.fetch_add(1, Ordering::Relaxed)
	}

	pub fn create_session(&self, link: Link, capabilities: u64) -> String {
		let session_id = format!("{:032x}", rand::rng().random::<u128>());
		let mut sessions = self.sessions.lock();
		sessions.retain(|_, record| record.detached_at.is_none_or(|at| at.elapsed() < DETACHED_TTL));
		sessions.insert(
			session_id.clone(),
			SessionRecord {
				seq: 0,
				buffer: VecDeque::new(),
				link: Some(link),
				detached_at: None,
				subscribed: HashSet::new(),
				capabilities,
			},
		);
		session_id
	}

	pub fn subscribe(&self, session_id: &str, guild_id: u64) {
		if let Some(record) = self.sessions.lock().get_mut(session_id) {
			record.subscribed.insert(guild_id);
		}
	}

	fn for_each_live(&self, mut visit: impl FnMut(&mut SessionRecord)) {
		let mut sessions = self.sessions.lock();
		sessions.retain(|_, record| record.detached_at.is_none_or(|at| at.elapsed() < DETACHED_TTL));
		sessions.values_mut().for_each(&mut visit);
	}

	pub fn broadcast_unless_passive(
		&self,
		guild_id: u64,
		name: &str,
		data: &Value,
		passive_v2: &Value,
		unread: &Value,
	) {
		let text = data.to_string();
		let passive_v2_text = OnceCell::new();
		let unread_text = OnceCell::new();
		self.for_each_live(|record| {
			if record.subscribed.contains(&guild_id) {
				record.push_event(name, &text);
			} else if record.capabilities & PASSIVE_GUILD_UPDATE_V2 != 0 {
				record.push_event(
					"PASSIVE_UPDATE_V2",
					passive_v2_text.get_or_init(|| passive_v2.to_string()),
				);
			} else {
				record.push_event(
					"CHANNEL_UNREAD_UPDATE",
					unread_text.get_or_init(|| unread.to_string()),
				);
			}
		});
	}

	pub fn broadcast_batched(&self, name: &str, batch: &Value, singles: &[Value]) {
		let text = batch.to_string();
		let single_texts: Vec<String> = singles.iter().map(Value::to_string).collect();
		self.for_each_live(|record| {
			if record.capabilities & DEBOUNCE_MESSAGE_REACTIONS != 0 {
				record.push_event(name, &text);
			} else {
				for single in &single_texts {
					record.push_event("MESSAGE_REACTION_ADD", single);
				}
			}
		});
	}

	pub fn expire_all(&self, code: u16) {
		for (_, record) in self.sessions.lock().drain() {
			if let Some(link) = record.link {
				let _ = link.tx.send(Outgoing::Close(code));
			}
		}
	}

	pub fn dispatch_to(&self, session_id: &str, name: &str, data: &Value) {
		let text = data.to_string();
		if let Some(record) = self.sessions.lock().get_mut(session_id) {
			record.push_event(name, &text);
		}
	}

	pub fn broadcast(&self, name: &str, data: &Value) {
		let text = data.to_string();
		self.for_each_live(|record| record.push_event(name, &text));
	}

	pub fn resume(&self, session_id: &str, seq: u64, link: Link) -> bool {
		let mut sessions = self.sessions.lock();
		let Some(record) = sessions.get_mut(session_id) else {
			return false;
		};
		let oldest = record.buffer.front().map_or(record.seq + 1, |(first, _)| *first);
		if seq > record.seq || seq + 1 < oldest {
			return false;
		}
		if let Some(previous) = record.link.replace(link.clone()) {
			let _ = previous.tx.send(Outgoing::Close(4000));
		}
		record.detached_at = None;
		for (_, frame) in record.buffer.iter().filter(|(frame_seq, _)| *frame_seq > seq) {
			let _ = link.tx.send(Outgoing::Frame(frame.clone()));
		}
		record.push_event("RESUMED", "{}");
		true
	}

	pub fn detach(&self, session_id: &str, link_id: u64) {
		if let Some(record) = self.sessions.lock().get_mut(session_id)
			&& record.link.as_ref().is_some_and(|link| link.id == link_id)
		{
			record.link = None;
			record.detached_at = Some(Instant::now());
		}
	}

	pub fn close_all(&self, code: u16) {
		for record in self.sessions.lock().values() {
			if let Some(link) = &record.link {
				let _ = link.tx.send(Outgoing::Close(code));
			}
		}
	}
}
