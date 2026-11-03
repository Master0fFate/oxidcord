use std::collections::HashMap;

use protocol::Snowflake;
use protocol::gateway::Opcode;
use serde_json::{Map, Value, json};

use crate::frames;

pub(crate) struct GuildSubscription {
	pub typing: bool,
	pub threads: bool,
	pub activities: bool,
	pub channels: HashMap<Snowflake, Vec<[u32; 2]>>,
}

impl GuildSubscription {
	fn absorb(&mut self, later: Self) {
		self.typing |= later.typing;
		self.threads |= later.threads;
		self.activities |= later.activities;
		self.channels = later.channels;
	}

	fn into_json(self) -> Value {
		let channels: Map<String, Value> = self
			.channels
			.into_iter()
			.map(|(channel_id, ranges)| (channel_id.to_string(), json!(ranges)))
			.collect();
		json!({
			"typing": self.typing,
			"threads": self.threads,
			"activities": self.activities,
			"members": [],
			"channels": channels,
		})
	}
}

#[derive(Default)]
pub(crate) struct Subscriptions(HashMap<Snowflake, GuildSubscription>);

impl Subscriptions {
	pub fn add(&mut self, guild_id: Snowflake, subscription: GuildSubscription) {
		match self.0.get_mut(&guild_id) {
			Some(earlier) => earlier.absorb(subscription),
			None => {
				self.0.insert(guild_id, subscription);
			}
		}
	}

	pub fn into_frame(self) -> String {
		let subscriptions: Map<String, Value> = self
			.0
			.into_iter()
			.map(|(guild_id, subscription)| (guild_id.to_string(), subscription.into_json()))
			.collect();
		frames::command(
			Opcode::GuildSubscriptionsBulk,
			json!({ "subscriptions": subscriptions }),
		)
	}
}
