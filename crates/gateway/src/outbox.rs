use std::collections::VecDeque;

use protocol::Snowflake;
use tokio::sync::mpsc;

use crate::limiter::SendWindow;
use crate::subscription::{GuildSubscription, Subscriptions};

const SEND_RESERVE: usize = 10;

pub(crate) enum Queued {
	Frame(String),
	Subscribe {
		guild_id: Snowflake,
		subscription: GuildSubscription,
	},
}

pub(crate) struct Outbox {
	commands: mpsc::Receiver<Queued>,
	pending: VecDeque<Queued>,
}

impl Outbox {
	pub fn new(commands: mpsc::Receiver<Queued>) -> Self {
		Self {
			commands,
			pending: VecDeque::new(),
		}
	}

	pub async fn next(&mut self, window: &mut SendWindow) -> Option<String> {
		window.room(SEND_RESERVE).await;
		if self.pending.is_empty() {
			let queued = self.commands.recv().await?;
			self.pending.push_back(queued);
		}
		while let Ok(queued) = self.commands.try_recv() {
			self.pending.push_back(queued);
		}
		Some(match self.pending.pop_front()? {
			Queued::Frame(frame) => frame,
			Queued::Subscribe {
				guild_id,
				subscription,
			} => self.merge_subscriptions(guild_id, subscription),
		})
	}

	pub fn discard_queued(&mut self) {
		self.pending.clear();
		while self.commands.try_recv().is_ok() {}
	}

	fn merge_subscriptions(&mut self, guild_id: Snowflake, subscription: GuildSubscription) -> String {
		let mut merged = Subscriptions::default();
		merged.add(guild_id, subscription);
		let mut rest = VecDeque::with_capacity(self.pending.len());
		for queued in self.pending.drain(..) {
			match queued {
				Queued::Subscribe {
					guild_id,
					subscription,
				} => merged.add(guild_id, subscription),
				other => rest.push_back(other),
			}
		}
		self.pending = rest;
		merged.into_frame()
	}
}
