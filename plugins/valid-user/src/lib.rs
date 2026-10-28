use std::collections::{HashSet, VecDeque};

use plugin_sdk::model::MessageFull;
use plugin_sdk::patch::Patch;
use plugin_sdk::{HostError, Md, MessageRef, Plugin, PluginExt, actions, data, export_plugin, log, timer};

const LOAD_TAG: &str = "load";
const LOAD_DELAY_MS: u32 = 400;
const RETRY_DELAY_MS: u32 = 5_000;
const LOADS_PER_TICK: usize = 2;
const MAX_PENDING: usize = 32;
const MAX_WAITING: usize = 8;
const MAX_ATTEMPTED: usize = 1024;

struct Pending {
	user_id: u64,
	messages: Vec<MessageRef>,
}

#[derive(Default)]
struct ValidUser {
	pending: VecDeque<Pending>,
	attempted: HashSet<u64>,
	armed: bool,
}

fn collect_mentions(nodes: &[Md], found: &mut Vec<u64>) {
	for node in nodes {
		match node {
			Md::UserMention(id) => found.push(*id),
			Md::Bold(children)
			| Md::Italic(children)
			| Md::Underline(children)
			| Md::Strike(children)
			| Md::Spoiler(children)
			| Md::Quote(children)
			| Md::Subtext(children)
			| Md::Heading { children, .. }
			| Md::Styled { children, .. } => collect_mentions(children, found),
			Md::Link {
				text: Some(children), ..
			} => collect_mentions(children, found),
			Md::List { items, .. } => {
				for item in items {
					collect_mentions(item, found);
				}
			}
			_ => {}
		}
	}
}

impl ValidUser {
	fn arm(&mut self, delay_ms: u32) {
		if self.armed {
			return;
		}
		match timer::once(delay_ms, LOAD_TAG) {
			Ok(_) => self.armed = true,
			Err(error) => log::debug!("could not schedule the user load: {error:?}"),
		}
	}

	fn enqueue(&mut self, user_id: u64, message: MessageRef) {
		if let Some(entry) = self.pending.iter_mut().find(|entry| entry.user_id == user_id) {
			if entry.messages.len() < MAX_WAITING && !entry.messages.contains(&message) {
				entry.messages.push(message);
			}
			return;
		}
		if self.pending.len() >= MAX_PENDING {
			return;
		}
		self.pending.push_back(Pending {
			user_id,
			messages: vec![message],
		});
		self.arm(LOAD_DELAY_MS);
	}

	fn settle(&mut self, user_id: u64) {
		if self.attempted.len() >= MAX_ATTEMPTED {
			self.attempted.clear();
		}
		self.attempted.insert(user_id);
	}
}

impl Plugin for ValidUser {}

impl PluginExt for ValidUser {
	fn intercept_message(&mut self, msg: &MessageFull, content: Vec<Md>) -> Patch {
		let mut mentioned = Vec::new();
		collect_mentions(&content, &mut mentioned);
		let message = MessageRef {
			id: msg.view.id,
			channel_id: msg.view.channel_id,
			guild_id: msg.view.guild_id,
		};
		for user_id in mentioned {
			if !self.attempted.contains(&user_id) && data::fetch_user(user_id).is_err() {
				self.enqueue(user_id, message);
			}
		}
		Patch::keep()
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag != LOAD_TAG {
			return;
		}
		self.armed = false;
		let mut delay = LOAD_DELAY_MS;
		for _ in 0..LOADS_PER_TICK {
			let Some(entry) = self.pending.pop_front() else {
				break;
			};
			match data::fetch_user(entry.user_id) {
				Ok(_) => {
					self.settle(entry.user_id);
					for message in entry.messages {
						if let Err(error) = actions::invalidate_message(message) {
							log::debug!("could not refresh message {}: {error:?}", message.id);
						}
					}
				}
				Err(HostError::RateLimited) => {
					self.pending.push_front(entry);
					delay = RETRY_DELAY_MS;
					break;
				}
				Err(error) => {
					log::debug!("could not load user {}: {error:?}", entry.user_id);
					self.settle(entry.user_id);
				}
			}
		}
		if !self.pending.is_empty() {
			self.arm(delay);
		}
	}
}

export_plugin!(ValidUser, ext);
