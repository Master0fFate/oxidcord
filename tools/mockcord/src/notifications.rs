use chrono::Duration;
use serde_json::{Value, json};

use crate::fixtures::Fixtures;
use crate::ids::snowflake_at;
use crate::json::{parse_id, to_json};
use crate::state::Shared;

const FEED_MINUTES: [i64; 5] = [4, 18, 55, 190, 1440];

fn linked_message(fixtures: &Fixtures) -> Option<(u64, u64, String)> {
	let (channel_id, messages) = fixtures
		.messages
		.iter()
		.find(|(_, messages)| !messages.is_empty())?;
	let (message_id, message) = messages.iter().next_back()?;
	let content = message["content"].as_str().unwrap_or_default().to_owned();
	Some((*channel_id, *message_id, content))
}

pub fn seed_notifications(fixtures: &Fixtures) -> Vec<Value> {
	let other = to_json(&fixtures.second_user);
	let name = fixtures
		.second_user
		.global_name
		.clone()
		.unwrap_or_else(|| fixtures.second_user.username.clone());
	let guild_id = fixtures.guilds.first().map(|guild| guild.id.to_string());
	let (channel_id, message_id, content) = linked_message(fixtures).unwrap_or_default();
	let id_at = |position: usize| {
		let at = fixtures.anchor - Duration::minutes(FEED_MINUTES[position]);
		snowflake_at(at.timestamp_millis(), position as u64).to_string()
	};
	vec![
		json!({
			"id": id_at(0),
			"bundle_id": "friend-requests",
			"type": "friend_request_pending",
			"body": format!("**{name}** sent you a friend request"),
			"acked": false,
			"deeplink": "/channels/@me",
			"other_user": other,
			"completed": false,
			"disable_action": false,
			"is_voice_message": false,
		}),
		json!({
			"id": id_at(1),
			"bundle_id": "go-live",
			"type": "go_live_push",
			"body": format!("**{name}** started streaming"),
			"acked": false,
			"deeplink": "/channels/@me",
			"other_user": other,
			"completed": false,
			"disable_action": false,
			"is_voice_message": false,
		}),
		json!({
			"id": id_at(2),
			"bundle_id": "events",
			"type": "scheduled_guild_event_started",
			"body": "A scheduled event just started",
			"callout": "Community hangout",
			"acked": false,
			"deeplink": "/channels/@me",
			"guild_id": guild_id,
			"completed": false,
			"disable_action": false,
			"is_voice_message": false,
		}),
		json!({
			"id": id_at(3),
			"bundle_id": "missed",
			"type": "missed_messages",
			"body": "You have unread messages",
			"acked": false,
			"deeplink": "/channels/@me",
			"message_id": message_id.to_string(),
			"message_channel_id": channel_id.to_string(),
			"message_content": content,
			"completed": false,
			"disable_action": false,
			"is_voice_message": false,
		}),
		json!({
			"id": id_at(4),
			"bundle_id": "reactions",
			"type": "reaction_sent",
			"body": format!("**{name}** reacted to your message"),
			"acked": true,
			"deeplink": "/channels/@me",
			"other_user": other,
			"emoji_name": "\u{1f44d}",
			"message_id": message_id.to_string(),
			"message_channel_id": channel_id.to_string(),
			"message_content": content,
			"completed": false,
			"disable_action": false,
			"is_voice_message": false,
		}),
	]
}

impl Shared {
	pub fn notification_page(&self, after: Option<u64>, limit: usize) -> Value {
		let store = self.store.lock();
		let mut items: Vec<&Value> = store
			.notifications
			.iter()
			.filter(|item| after.is_none_or(|after| parse_id(&item["id"]).unwrap_or_default() < after))
			.collect();
		items.sort_unstable_by_key(|item| std::cmp::Reverse(parse_id(&item["id"]).unwrap_or_default()));
		let has_more = items.len() > limit;
		items.truncate(limit);
		let cursor = items.last().map(|item| item["id"].clone());
		json!({ "limit": limit, "items": items, "cursor": cursor, "has_more": has_more })
	}

	pub fn delete_notification(&self, id: u64) {
		let mut store = self.store.lock();
		let before = store.notifications.len();
		store
			.notifications
			.retain(|item| parse_id(&item["id"]) != Some(id));
		let removed = store.notifications.len() != before;
		drop(store);
		if removed {
			self.hub.broadcast(
				"NOTIFICATION_CENTER_ITEM_DELETE",
				&json!({ "id": id.to_string(), "user_id": self.fixtures.current_user.id.to_string() }),
			);
		}
	}

	pub fn ack_notifications(&self, ids: &[u64]) {
		let mut store = self.store.lock();
		let acked: Vec<u64> = store
			.notifications
			.iter_mut()
			.filter_map(|item| {
				let id = parse_id(&item["id"]).filter(|id| ids.contains(id))?;
				item["acked"] = Value::Bool(true);
				Some(id)
			})
			.collect();
		drop(store);
		for id in acked {
			self.hub
				.broadcast("NOTIFICATION_CENTER_ITEMS_ACK", &json!({ "id": id.to_string() }));
		}
	}
}
