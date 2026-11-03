use std::sync::Arc;
use std::time::{Duration, Instant};

use protocol::{InteractionAck, InteractionFailure, InteractionModal, Nonce, Snowflake};

use crate::changes::Changes;
use crate::state::State;

pub const INTERACTION_TTL: Duration = Duration::from_secs(15);
const TIMEOUT_REASON: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionPhase {
	Loading,
	Failed,
}

#[derive(Debug)]
pub(crate) struct PendingInteraction {
	channel_id: Snowflake,
	message_id: Option<Snowflake>,
	phase: InteractionPhase,
	until: Instant,
}

impl PendingInteraction {
	fn changes(&self) -> Changes {
		let mut changes = Changes {
			interactions: true,
			..Changes::default()
		};
		if self.message_id.is_some() {
			changes.messages.insert(self.channel_id);
		}
		changes
	}
}

#[derive(Debug, Clone)]
pub struct ModalPrompt {
	pub id: u64,
	pub modal: Arc<InteractionModal>,
}

impl State {
	pub fn message_interaction(&self, message_id: Snowflake) -> Option<InteractionPhase> {
		self.interactions
			.values()
			.find(|pending| pending.message_id == Some(message_id))
			.map(|pending| pending.phase)
	}

	pub fn interaction_modal(&self) -> Option<&ModalPrompt> {
		self.modal.as_ref()
	}

	pub fn interaction_wait(&self, now: Instant) -> Option<Duration> {
		self.interactions
			.values()
			.map(|pending| pending.until.saturating_duration_since(now))
			.min()
	}

	pub fn expire_interactions(&mut self, now: Instant) -> Changes {
		let mut changes = Changes::default();
		self.interactions.retain(|_, pending| {
			let alive = pending.until > now;
			if !alive {
				changes.merge(pending.changes());
			}
			alive
		});
		changes
	}

	pub(crate) fn interaction_started(
		&mut self,
		nonce: &Nonce,
		channel_id: Snowflake,
		message_id: Option<Snowflake>,
	) -> Changes {
		let pending = PendingInteraction {
			channel_id,
			message_id,
			phase: InteractionPhase::Loading,
			until: Instant::now() + INTERACTION_TTL,
		};
		let changes = pending.changes();
		self.interactions.insert(nonce.clone(), pending);
		changes
	}

	pub(crate) fn interaction_succeeded(&mut self, ack: &InteractionAck) -> Changes {
		ack.nonce
			.as_ref()
			.and_then(|nonce| self.interactions.remove(nonce))
			.map_or_else(Changes::default, |pending| pending.changes())
	}

	pub(crate) fn interaction_failed(&mut self, failure: &InteractionFailure) -> Changes {
		let Some(nonce) = &failure.nonce else {
			return Changes::default();
		};
		let Some(pending) = self.interactions.get_mut(nonce) else {
			return Changes::default();
		};
		if pending.message_id.is_some() {
			pending.phase = InteractionPhase::Failed;
			pending.until = Instant::now() + INTERACTION_TTL;
			return pending.changes();
		}
		let changes = pending.changes();
		self.interactions.remove(nonce);
		let reason = if failure.reason_code == TIMEOUT_REASON {
			"App did not respond"
		} else {
			"Interaction failed"
		};
		self.note_rejection(reason.to_owned());
		Changes {
			rejected: true,
			..changes
		}
	}

	pub(crate) fn interaction_modal_created(&mut self, modal: &InteractionModal) -> Changes {
		let mut changes = modal
			.nonce
			.as_ref()
			.and_then(|nonce| self.interactions.remove(nonce))
			.map_or_else(Changes::default, |pending| pending.changes());
		self.modal_count += 1;
		self.modal = Some(ModalPrompt {
			id: self.modal_count,
			modal: Arc::new(modal.clone()),
		});
		changes.interaction_modal = true;
		changes
	}

	pub(crate) fn ephemeral_dismissed(&mut self, channel_id: Snowflake, message_id: Snowflake) -> Changes {
		let mut changes = Changes::default();
		if self
			.messages
			.get_mut(&channel_id)
			.is_some_and(|cache| cache.remove(message_id))
		{
			changes.messages.insert(channel_id);
		}
		changes
	}
}
