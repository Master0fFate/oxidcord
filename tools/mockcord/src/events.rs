use std::collections::HashMap;

use chrono::{DateTime, Utc};
use protocol::{ScheduledEvent, ScheduledEventKind, ScheduledEventMetadata, ScheduledEventStatus, Snowflake};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::json::{parse_id, to_json};
use crate::state::{Shared, Store};

const RESPONSE_INTERESTED: i64 = 1;

fn parse_time(value: &Value) -> Option<DateTime<Utc>> {
	DateTime::parse_from_rfc3339(value.as_str()?)
		.ok()
		.map(|time| time.with_timezone(&Utc))
}

fn interest_payload(guild_id: u64, event_id: u64) -> Value {
	json!({
		"guild_scheduled_event_id": event_id.to_string(),
		"guild_id": guild_id.to_string(),
		"user_id": CURRENT_USER_ID.to_string(),
	})
}

fn rsvp_payload(event_id: u64) -> Value {
	json!({
		"guild_scheduled_event_id": event_id.to_string(),
		"user_id": CURRENT_USER_ID.to_string(),
		"response": RESPONSE_INTERESTED,
	})
}

fn allowed_transition(from: ScheduledEventStatus, to: ScheduledEventStatus) -> bool {
	matches!(
		(from, to),
		(ScheduledEventStatus::Scheduled, ScheduledEventStatus::Active)
			| (ScheduledEventStatus::Scheduled, ScheduledEventStatus::Canceled)
			| (ScheduledEventStatus::Active, ScheduledEventStatus::Completed)
	)
}

impl Store {
	pub fn open_scheduled_events(&self, guild_id: u64) -> Vec<&ScheduledEvent> {
		self.scheduled_events
			.get(&guild_id)
			.into_iter()
			.flatten()
			.filter(|event| event.is_open())
			.collect()
	}
}

fn find_event(
	events: &mut HashMap<u64, Vec<ScheduledEvent>>,
	guild_id: u64,
	event_id: u64,
) -> Result<&mut ScheduledEvent, ApiError> {
	events
		.get_mut(&guild_id)
		.and_then(|events| events.iter_mut().find(|event| event.id.0 == event_id))
		.ok_or_else(ApiError::not_found)
}

impl Shared {
	pub fn scheduled_events_json(&self, guild_id: u64) -> Result<Value, ApiError> {
		let store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		Ok(to_json(&store.open_scheduled_events(guild_id)))
	}

	pub fn event_rsvps_json(&self, guild_ids: &[u64]) -> Value {
		let store = self.store.lock();
		let rsvps: Vec<Value> = store
			.scheduled_events
			.iter()
			.filter(|(guild_id, _)| guild_ids.contains(guild_id))
			.flat_map(|(_, events)| events)
			.filter(|event| event.is_open() && store.event_interest.contains(&event.id.0))
			.map(|event| rsvp_payload(event.id.0))
			.collect();
		json!(rsvps)
	}

	pub fn create_scheduled_event(&self, guild_id: u64, body: &Value) -> Result<Value, ApiError> {
		let name = body["name"]
			.as_str()
			.filter(|name| !name.is_empty())
			.ok_or_else(|| ApiError::invalid_body("name is required"))?;
		let starts_at = parse_time(&body["scheduled_start_time"])
			.ok_or_else(|| ApiError::invalid_body("scheduled_start_time is required"))?;
		let kind = ScheduledEventKind::from(body["entity_type"].as_i64().unwrap_or_default());
		if kind.is_unknown() {
			return Err(ApiError::invalid_body("entity_type is invalid"));
		}
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let id = store.next_snowflake();
		let event = ScheduledEvent {
			id: Snowflake(id),
			guild_id: Snowflake(guild_id),
			channel_id: parse_id(&body["channel_id"]).map(Snowflake),
			creator_id: Some(Snowflake(CURRENT_USER_ID)),
			name: name.to_owned(),
			description: body["description"].as_str().map(str::to_owned),
			scheduled_start_time: Some(starts_at),
			scheduled_end_time: parse_time(&body["scheduled_end_time"]),
			status: ScheduledEventStatus::Scheduled,
			entity_type: kind,
			entity_metadata: body["entity_metadata"]["location"].as_str().map(|location| {
				ScheduledEventMetadata {
					location: Some(location.to_owned()),
				}
			}),
			user_count: Some(0),
			..ScheduledEvent::default()
		};
		let value = to_json(&event);
		store.scheduled_events.entry(guild_id).or_default().push(event);
		drop(store);
		self.hub.broadcast("GUILD_SCHEDULED_EVENT_CREATE", &value);
		self.set_event_interest(guild_id, id, true)?;
		Ok(value)
	}

	pub fn modify_scheduled_event(
		&self,
		guild_id: u64,
		event_id: u64,
		body: &Value,
	) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let event = find_event(&mut store.scheduled_events, guild_id, event_id)?;
		if let Some(status) = body["status"].as_i64() {
			let next = ScheduledEventStatus::from(status);
			if !allowed_transition(event.status, next) {
				return Err(ApiError::invalid_body("invalid status transition"));
			}
			event.status = next;
		}
		if let Some(name) = body["name"].as_str() {
			event.name = name.to_owned();
		}
		if let Some(description) = body.get("description") {
			event.description = description.as_str().map(str::to_owned);
		}
		if let Some(channel_id) = parse_id(&body["channel_id"]) {
			event.channel_id = Some(Snowflake(channel_id));
		}
		if let Some(location) = body["entity_metadata"]["location"].as_str() {
			event.entity_metadata = Some(ScheduledEventMetadata {
				location: Some(location.to_owned()),
			});
		}
		if let Some(starts_at) = parse_time(&body["scheduled_start_time"]) {
			event.scheduled_start_time = Some(starts_at);
		}
		if let Some(ends_at) = parse_time(&body["scheduled_end_time"]) {
			event.scheduled_end_time = Some(ends_at);
		}
		let value = to_json(&*event);
		drop(store);
		self.hub.broadcast("GUILD_SCHEDULED_EVENT_UPDATE", &value);
		Ok(value)
	}

	pub fn delete_scheduled_event(&self, guild_id: u64, event_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let events = store
			.scheduled_events
			.get_mut(&guild_id)
			.ok_or_else(ApiError::not_found)?;
		let position = events
			.iter()
			.position(|event| event.id.0 == event_id)
			.ok_or_else(ApiError::not_found)?;
		let value = to_json(&events.remove(position));
		store.event_interest.remove(&event_id);
		drop(store);
		self.hub.broadcast("GUILD_SCHEDULED_EVENT_DELETE", &value);
		Ok(())
	}

	pub fn invite_event_json(&self, guild_id: u64, event_id: u64) -> Option<Value> {
		let store = self.store.lock();
		store
			.open_scheduled_events(guild_id)
			.into_iter()
			.find(|event| event.id.0 == event_id)
			.map(to_json)
	}

	pub fn first_open_event_json(&self, guild_id: u64) -> Option<Value> {
		let store = self.store.lock();
		store
			.open_scheduled_events(guild_id)
			.into_iter()
			.next()
			.map(to_json)
	}

	pub fn set_event_interest(
		&self,
		guild_id: u64,
		event_id: u64,
		interested: bool,
	) -> Result<Value, ApiError> {
		let mut guard = self.store.lock();
		self.live_guild(&guard, guild_id)?;
		let store = &mut *guard;
		let event = find_event(&mut store.scheduled_events, guild_id, event_id)?;
		let changed = if interested {
			store.event_interest.insert(event_id)
		} else {
			store.event_interest.remove(&event_id)
		};
		if changed {
			let count = event.user_count.unwrap_or_default();
			event.user_count = Some(if interested { count + 1 } else { (count - 1).max(0) });
		}
		drop(guard);
		if changed {
			let name = if interested {
				"GUILD_SCHEDULED_EVENT_USER_ADD"
			} else {
				"GUILD_SCHEDULED_EVENT_USER_REMOVE"
			};
			self.hub.broadcast(name, &interest_payload(guild_id, event_id));
		}
		Ok(rsvp_payload(event_id))
	}

	pub fn next_scheduled_event(&self) -> Option<(u64, u64)> {
		let store = self.store.lock();
		self.fixtures.guilds.iter().find_map(|guild| {
			store
				.open_scheduled_events(guild.id)
				.into_iter()
				.find(|event| event.status == ScheduledEventStatus::Scheduled)
				.map(|event| (guild.id, event.id.0))
		})
	}
}
