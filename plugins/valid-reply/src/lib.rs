use std::collections::{HashMap, HashSet, VecDeque};

use plugin_sdk::model::{ButtonVariant, MessageFull, Space, TextSize, Tone, ViewEvent, ViewEventKind};
use plugin_sdk::patch::Patch;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	HostError, Md, MessageRef, Plugin, PluginExt, Reply, actions, client, data, export_plugin, log, timer,
};

const ACCESSORY_POINT: &str = "message-accessory";
const JUMP_PREFIX: &str = "jump:";
const LOAD_TAG: &str = "load";
const LOAD_DELAY_MS: u32 = 400;
const RETRY_DELAY_MS: u32 = 2_000;
const LOADS_PER_TICK: usize = 2;
const MAX_PENDING: usize = 16;
const MAX_WAITING: usize = 8;
const MAX_PREVIEWS: usize = 256;
const MAX_ATTEMPTED: usize = 1024;
const SNIPPET_CHARS: usize = 140;

struct Preview {
	author: String,
	text: String,
}

struct Pending {
	target: MessageRef,
	replies: Vec<MessageRef>,
}

#[derive(Default)]
struct ValidReply {
	previews: HashMap<u64, Preview>,
	order: VecDeque<u64>,
	pending: VecDeque<Pending>,
	attempted: HashSet<u64>,
	armed: bool,
}

fn preview_of(full: &MessageFull) -> Preview {
	let flat: String = full.view.content.split_whitespace().collect::<Vec<_>>().join(" ");
	let text = if flat.is_empty() {
		if full.attachments.is_empty() {
			"No text".to_owned()
		} else {
			"Attachment".to_owned()
		}
	} else {
		flat.chars().take(SNIPPET_CHARS).collect()
	};
	Preview {
		author: full.view.author.display_name.clone(),
		text,
	}
}

fn accessory(target: u64, preview: &Preview) -> Node {
	view::row()
		.gap(Space::Xs)
		.child(view::icon("reply").tone(Tone::Muted))
		.child(view::text(preview.author.clone()).bold())
		.child(view::text(preview.text.clone()).size(TextSize::Xs).muted())
		.child(
			view::button(format!("{JUMP_PREFIX}{target}"), "Jump")
				.icon("arrow-up-right")
				.variant(ButtonVariant::Link),
		)
}

impl ValidReply {
	fn remember(&mut self, target: u64, preview: Preview) {
		if self.previews.insert(target, preview).is_none() {
			self.order.push_back(target);
			if self.order.len() > MAX_PREVIEWS
				&& let Some(oldest) = self.order.pop_front()
			{
				self.previews.remove(&oldest);
			}
		}
	}

	fn settle(&mut self, target: u64) {
		if self.attempted.len() >= MAX_ATTEMPTED {
			self.attempted.clear();
		}
		self.attempted.insert(target);
	}

	fn arm(&mut self, delay_ms: u32) {
		if self.armed {
			return;
		}
		match timer::once(delay_ms, LOAD_TAG) {
			Ok(_) => self.armed = true,
			Err(error) => log::debug!("could not schedule the reply load: {error:?}"),
		}
	}

	fn enqueue(&mut self, target: MessageRef, reply: MessageRef) {
		if let Some(entry) = self.pending.iter_mut().find(|entry| entry.target.id == target.id) {
			if entry.replies.len() < MAX_WAITING && !entry.replies.contains(&reply) {
				entry.replies.push(reply);
			}
			return;
		}
		if self.pending.len() >= MAX_PENDING {
			return;
		}
		self.pending.push_back(Pending {
			target,
			replies: vec![reply],
		});
		self.arm(LOAD_DELAY_MS);
	}
}

impl Plugin for ValidReply {}

impl PluginExt for ValidReply {
	fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
		let Some(target_id) = msg.view.reply_to else {
			return Patch::keep();
		};
		let channel_id = msg.view.channel_id;
		if matches!(client::get_message(channel_id, target_id), Ok(Some(_))) {
			return Patch::keep();
		}
		if let Some(preview) = self.previews.get(&target_id) {
			return Patch::keep().accessory([accessory(target_id, preview)]);
		}
		if !self.attempted.contains(&target_id) {
			let target = MessageRef {
				id: target_id,
				channel_id,
				guild_id: msg.view.guild_id,
			};
			let reply = MessageRef {
				id: msg.view.id,
				channel_id,
				guild_id: msg.view.guild_id,
			};
			self.enqueue(target, reply);
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
			let target = entry.target;
			match data::fetch_message(target.channel_id, target.id) {
				Ok(found) => {
					self.settle(target.id);
					if let Some(full) = found {
						self.remember(target.id, preview_of(&full));
						for reply in entry.replies {
							if let Err(error) = actions::invalidate_message(reply) {
								log::debug!("could not refresh message {}: {error:?}", reply.id);
							}
						}
					}
				}
				Err(HostError::RateLimited) => {
					self.pending.push_front(entry);
					delay = RETRY_DELAY_MS;
					break;
				}
				Err(error) => {
					log::debug!("could not load message {}: {error:?}", target.id);
					self.settle(target.id);
				}
			}
		}
		if !self.pending.is_empty() {
			self.arm(delay);
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click || event.point != ACCESSORY_POINT {
			return Reply::none();
		}
		let target = event
			.node
			.strip_prefix(JUMP_PREFIX)
			.and_then(|id| id.parse::<u64>().ok());
		if let (Some(id), Some(channel_id)) = (target, event.context.channel_id)
			&& let Err(error) = actions::jump_to_message(MessageRef {
				id,
				channel_id,
				guild_id: event.context.guild_id,
			}) {
			log::debug!("could not jump to message {id}: {error:?}");
		}
		Reply::none()
	}
}

export_plugin!(ValidReply, ext);
