use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use protocol::{
	Message, MessageUpdate, Nonce, PartialEmoji, PollAnswerCount, PollResults, Reaction,
	ReactionCountDetails, Snowflake,
};

use crate::event::LoadDirection;
use crate::views::RenderedMessage;

pub const MESSAGE_CAP: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFile {
	pub name: String,
	pub size: u64,
}

#[derive(Debug, Clone)]
pub struct PendingMessage {
	pub message: Arc<Message>,
	pub files: Arc<[PendingFile]>,
	pub uploaded: u64,
	pub failed: bool,
	pub retryable: bool,
	pub failure_reason: Option<String>,
	pub plugin: Option<String>,
	pub altered_by: Option<String>,
	pub rendered: Option<Arc<RenderedMessage>>,
	pub sent_at: Instant,
}

#[derive(Debug, Default)]
pub struct ChannelMessages {
	pub ordered: Vec<Arc<Message>>,
	pub by_id: HashMap<Snowflake, Arc<Message>>,
	pub rendered: HashMap<Snowflake, Arc<RenderedMessage>>,
	pub reached_start: bool,
	pub pending: Vec<PendingMessage>,
	pub gaps: Vec<(Snowflake, Snowflake)>,
	deleted: HashSet<Snowflake>,
	latency: HashMap<Snowflake, u32>,
	detached: bool,
	stale: bool,
}

impl ChannelMessages {
	pub fn is_deleted(&self, id: Snowflake) -> bool {
		self.deleted.contains(&id)
	}

	pub fn latency_ms(&self, id: Snowflake) -> Option<u32> {
		self.latency.get(&id).copied()
	}

	pub fn record_latency(&mut self, id: Snowflake, millis: u32) {
		self.latency.insert(id, millis);
	}

	pub fn is_hidden(&self, id: Snowflake) -> bool {
		let deleted = self.is_deleted(id);
		self.rendered
			.get(&id)
			.map_or(deleted, |rendered| rendered.hidden || rendered.deleted != deleted)
	}

	pub fn mark_deleted(&mut self, id: Snowflake) -> Option<Arc<Message>> {
		let message = self.by_id.get(&id).cloned()?;
		self.deleted.insert(id).then_some(message)
	}

	pub fn drop_deleted(&mut self) -> bool {
		let deleted = std::mem::take(&mut self.deleted);
		for &id in &deleted {
			self.remove(id);
		}
		!deleted.is_empty()
	}

	pub fn newest_id(&self) -> Option<Snowflake> {
		self.ordered.last().map(|message| message.id)
	}

	pub fn is_detached(&self) -> bool {
		self.detached
	}

	pub fn is_stale(&self) -> bool {
		self.stale
	}

	pub fn mark_stale(&mut self) {
		if self.ordered.is_empty() {
			return;
		}
		self.stale = true;
		self.detached = true;
		self.reset_tail_gap(None);
	}

	pub fn push_live(&mut self, message: Arc<Message>) -> bool {
		if self.detached {
			self.extend_tail_gap(message.id);
			return false;
		}
		self.insert(message);
		self.evict_oldest();
		true
	}

	pub fn merge_loaded(
		&mut self,
		messages: Vec<Arc<Message>>,
		direction: LoadDirection,
		reached_start: bool,
		channel_last: Option<Snowflake>,
	) {
		let was_empty = self.ordered.is_empty();
		let received = messages.len();
		if direction == LoadDirection::Around {
			self.ordered.clear();
			self.by_id.clear();
			self.rendered.clear();
			self.deleted.clear();
			self.latency.clear();
			self.gaps.clear();
			self.stale = false;
		}
		self.drop_confirmed_pending(&messages);
		for message in messages {
			self.insert(message);
		}
		match direction {
			LoadDirection::History => {
				self.reached_start = reached_start;
				if was_empty {
					self.detached = false;
				}
				self.evict_newest();
			}
			LoadDirection::Around => {
				self.reached_start = reached_start;
				self.detached = self.behind(channel_last);
				self.evict_newest();
			}
			LoadDirection::Newer => {
				self.detached = received != 0 && self.behind(channel_last);
				self.evict_oldest();
			}
		}
		self.reset_tail_gap(channel_last);
	}

	pub fn remove(&mut self, id: Snowflake) -> bool {
		let Ok(index) = self.ordered.binary_search_by_key(&id, |message| message.id) else {
			return false;
		};
		self.ordered.remove(index);
		self.by_id.remove(&id);
		self.rendered.remove(&id);
		self.deleted.remove(&id);
		self.latency.remove(&id);
		true
	}

	pub fn modify(&mut self, id: Snowflake, edit: impl FnOnce(&mut Message)) -> bool {
		let Ok(index) = self.ordered.binary_search_by_key(&id, |message| message.id) else {
			return false;
		};
		let mut message = (*self.ordered[index]).clone();
		edit(&mut message);
		let message = Arc::new(message);
		self.by_id.insert(id, Arc::clone(&message));
		self.ordered[index] = message;
		true
	}

	pub fn add_pending(
		&mut self,
		message: Arc<Message>,
		files: Arc<[PendingFile]>,
		plugin: Option<String>,
		rendered: Arc<RenderedMessage>,
	) {
		self.pending.push(PendingMessage {
			message,
			files,
			uploaded: 0,
			failed: false,
			retryable: false,
			failure_reason: None,
			plugin,
			altered_by: None,
			rendered: Some(rendered),
			sent_at: Instant::now(),
		});
	}

	pub fn take_pending(&mut self, nonce: &Nonce) -> Option<PendingMessage> {
		let index = self
			.pending
			.iter()
			.position(|pending| pending.message.nonce.as_ref() == Some(nonce))?;
		Some(self.pending.remove(index))
	}

	pub fn remove_pending(&mut self, nonce: &Nonce) -> bool {
		self.take_pending(nonce).is_some()
	}

	pub fn has_pending(&self, nonce: &Nonce) -> bool {
		self.pending
			.iter()
			.any(|pending| pending.message.nonce.as_ref() == Some(nonce))
	}

	pub fn fail_pending(&mut self, nonce: &Nonce, reason: Option<String>, retryable: bool) -> bool {
		match self.pending_mut(nonce) {
			Some(pending) => {
				pending.failed = true;
				pending.retryable = retryable;
				pending.failure_reason = reason;
				true
			}
			None => false,
		}
	}

	pub fn retry_pending(&mut self, nonce: &Nonce) -> bool {
		match self.pending_mut(nonce) {
			Some(pending) => {
				pending.failed = false;
				pending.retryable = false;
				pending.failure_reason = None;
				pending.uploaded = 0;
				pending.sent_at = Instant::now();
				true
			}
			None => false,
		}
	}

	pub fn upload_progress(&mut self, nonce: &Nonce, uploaded: u64) -> bool {
		match self.pending_mut(nonce) {
			Some(pending) => {
				pending.uploaded = uploaded;
				true
			}
			None => false,
		}
	}

	pub fn alter_pending(&mut self, nonce: &Nonce, content: String, by: String) -> bool {
		let Some(pending) = self.pending_mut(nonce) else {
			return false;
		};
		let mut message = (*pending.message).clone();
		pending.rendered = Some(Arc::new(RenderedMessage::plain(&content, false, 0)));
		message.content = content;
		pending.message = Arc::new(message);
		pending.altered_by = Some(by);
		true
	}

	fn drop_confirmed_pending(&mut self, loaded: &[Arc<Message>]) {
		if self.pending.is_empty() {
			return;
		}
		self.pending.retain(|pending| {
			!loaded.iter().any(|message| {
				message.nonce.is_some()
					&& message.nonce == pending.message.nonce
					&& message.author.id == pending.message.author.id
			})
		});
	}

	fn pending_mut(&mut self, nonce: &Nonce) -> Option<&mut PendingMessage> {
		self.pending
			.iter_mut()
			.find(|pending| pending.message.nonce.as_ref() == Some(nonce))
	}

	fn insert(&mut self, message: Arc<Message>) {
		self.by_id.insert(message.id, Arc::clone(&message));
		match self
			.ordered
			.binary_search_by_key(&message.id, |existing| existing.id)
		{
			Ok(index) => self.ordered[index] = message,
			Err(index) => self.ordered.insert(index, message),
		}
	}

	fn behind(&self, channel_last: Option<Snowflake>) -> bool {
		match (self.newest_id(), channel_last) {
			(Some(newest), Some(last)) => newest < last,
			_ => false,
		}
	}

	fn evict_oldest(&mut self) {
		let excess = self.ordered.len().saturating_sub(MESSAGE_CAP);
		if excess == 0 {
			return;
		}
		for message in self.ordered.drain(..excess) {
			self.by_id.remove(&message.id);
			self.rendered.remove(&message.id);
			self.deleted.remove(&message.id);
			self.latency.remove(&message.id);
		}
		self.reached_start = false;
	}

	fn evict_newest(&mut self) {
		if self.ordered.len() <= MESSAGE_CAP {
			return;
		}
		for message in self.ordered.drain(MESSAGE_CAP..) {
			self.by_id.remove(&message.id);
			self.rendered.remove(&message.id);
			self.deleted.remove(&message.id);
			self.latency.remove(&message.id);
		}
		self.detached = true;
	}

	fn extend_tail_gap(&mut self, id: Snowflake) {
		let newest = self.newest_id().unwrap_or_default();
		match self.gaps.last_mut() {
			Some(gap) if gap.0 == newest => gap.1 = gap.1.max(id),
			_ => self.gaps.push((newest, id)),
		}
	}

	fn reset_tail_gap(&mut self, upper: Option<Snowflake>) {
		let previous = self.gaps.last().map(|gap| gap.1);
		self.gaps.clear();
		if !self.detached {
			return;
		}
		let newest = self.newest_id().unwrap_or_default();
		let upper = upper.max(previous).unwrap_or(newest).max(newest);
		self.gaps.push((newest, upper));
	}
}

pub fn apply_update(message: &mut Message, update: &MessageUpdate) {
	if let Some(author) = &update.author {
		message.author = author.clone();
	}
	if let Some(member) = &update.member {
		message.member = Some(member.clone());
	}
	if let Some(content) = &update.content {
		message.content.clone_from(content);
	}
	if update.edited_timestamp.is_some() {
		message.edited_timestamp = update.edited_timestamp;
	}
	if let Some(tts) = update.tts {
		message.tts = tts;
	}
	if let Some(everyone) = update.mention_everyone {
		message.mention_everyone = everyone;
	}
	if let Some(mentions) = &update.mentions {
		message.mentions = mentions.clone();
	}
	if let Some(roles) = &update.mention_roles {
		message.mention_roles.clone_from(roles);
	}
	if let Some(channels) = &update.mention_channels {
		message.mention_channels = channels.clone();
	}
	if let Some(attachments) = &update.attachments {
		message.attachments = attachments.clone();
	}
	if let Some(embeds) = &update.embeds {
		message.embeds = embeds.clone();
	}
	if let Some(reactions) = &update.reactions {
		message.reactions = reactions.clone();
	}
	if let Some(pinned) = update.pinned {
		message.pinned = pinned;
	}
	if let Some(kind) = update.kind {
		message.kind = kind;
	}
	if let Some(flags) = update.flags {
		message.flags = flags;
	}
	if let Some(reference) = &update.message_reference {
		message.message_reference = Some(reference.clone());
	}
	if let Some(referenced) = &update.referenced_message {
		message.referenced_message = Some(referenced.clone());
	}
	if let Some(components) = &update.components {
		message.components.clone_from(components);
	}
	if let Some(stickers) = &update.sticker_items {
		message.sticker_items = stickers.clone();
	}
	if let Some(poll) = &update.poll {
		message.poll = Some(poll.clone());
	}
	if let Some(application) = &update.application {
		message.application = Some(application.clone());
	}
	if update.application_id.is_some() {
		message.application_id = update.application_id;
	}
	if let Some(snapshots) = &update.message_snapshots {
		message.message_snapshots = snapshots.clone();
	}
	if update.interaction_metadata.is_some() {
		message
			.interaction_metadata
			.clone_from(&update.interaction_metadata);
	}
	if update.interaction.is_some() {
		message.interaction.clone_from(&update.interaction);
	}
	if let Some(thread) = &update.thread {
		message.thread = Some(thread.clone());
	}
	if update.position.is_some() {
		message.position = update.position;
	}
	if let Some(call) = &update.call {
		message.call = Some(call.clone());
	}
}

fn same_emoji(a: &PartialEmoji, b: &PartialEmoji) -> bool {
	a.id == b.id && (a.id.is_some() || a.name == b.name)
}

fn adjust_count(reaction: &mut Reaction, burst: bool, delta: i64) {
	if delta < 0 && reaction.count_details.is_none() {
		reaction.count = (reaction.count + delta).max(0);
		return;
	}
	let details = reaction.count_details.get_or_insert(ReactionCountDetails {
		burst: 0,
		normal: reaction.count,
	});
	let side = if burst {
		&mut details.burst
	} else {
		&mut details.normal
	};
	*side = (*side + delta).max(0);
	reaction.count = details.burst + details.normal;
}

pub struct ReactionChange<'a> {
	pub emoji: &'a PartialEmoji,
	pub mine: bool,
	pub burst: bool,
	pub burst_colors: &'a [String],
}

pub fn add_reaction(message: &mut Message, change: &ReactionChange) {
	let position = message
		.reactions
		.iter()
		.position(|reaction| same_emoji(&reaction.emoji, change.emoji))
		.unwrap_or_else(|| {
			message.reactions.push(Reaction {
				emoji: change.emoji.clone(),
				..Reaction::default()
			});
			message.reactions.len() - 1
		});
	let reaction = &mut message.reactions[position];
	adjust_count(reaction, change.burst, 1);
	if change.burst {
		reaction.me_burst |= change.mine;
		if reaction.burst_colors.is_empty() {
			reaction.burst_colors = change.burst_colors.to_vec();
		}
	} else {
		reaction.me |= change.mine;
	}
}

pub fn apply_poll_vote(message: &mut Message, answer_id: i64, delta: i64, mine: bool) {
	let Some(poll) = message.poll.as_mut() else {
		return;
	};
	let counts = &mut poll
		.results
		.get_or_insert_with(PollResults::default)
		.answer_counts;
	let position = counts
		.iter()
		.position(|entry| entry.id == answer_id)
		.unwrap_or_else(|| {
			counts.push(PollAnswerCount {
				id: answer_id,
				..PollAnswerCount::default()
			});
			counts.len() - 1
		});
	let entry = &mut counts[position];
	entry.count = (entry.count + delta).max(0);
	if mine {
		entry.me_voted = delta > 0;
	}
}

pub fn remove_reaction(message: &mut Message, change: &ReactionChange) {
	if let Some(reaction) = message
		.reactions
		.iter_mut()
		.find(|reaction| same_emoji(&reaction.emoji, change.emoji))
	{
		adjust_count(reaction, change.burst, -1);
		if change.mine {
			if change.burst {
				reaction.me_burst = false;
			} else {
				reaction.me = false;
			}
		}
	}
	message.reactions.retain(|reaction| reaction.count > 0);
}

pub fn remove_emoji(message: &mut Message, emoji: &PartialEmoji) {
	message
		.reactions
		.retain(|reaction| !same_emoji(&reaction.emoji, emoji));
}
