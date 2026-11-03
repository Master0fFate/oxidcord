use std::time::Instant;

use protocol::{ScheduledEvent, ScheduledEventStatus, ScheduledEventUser, Snowflake};

use crate::changes::Changes;
use crate::state::State;

fn events_loaded(guild_id: Snowflake) -> Changes {
	let mut changes = Changes::default();
	changes.loaded.insert(guild_id);
	changes
}

fn events_changed(guild_id: Snowflake) -> Changes {
	let mut changes = events_loaded(guild_id);
	changes.channel_lists.insert(guild_id);
	changes
}

impl State {
	pub fn scheduled_events(&self, guild_id: Snowflake) -> &[ScheduledEvent] {
		self.guilds
			.get(&guild_id)
			.map_or(&[][..], |guild| guild.scheduled_events.as_slice())
	}

	pub fn event_interested(&self, guild_id: Snowflake, event_id: Snowflake) -> bool {
		self.guilds
			.get(&guild_id)
			.is_some_and(|guild| guild.interested_events.contains(&event_id))
	}

	pub fn event_live_in(&self, guild_id: Snowflake, channel_id: Snowflake) -> bool {
		self.scheduled_events(guild_id)
			.iter()
			.any(|event| event.status == ScheduledEventStatus::Active && event.channel_id == Some(channel_id))
	}

	pub(crate) fn scheduled_event_upserted(&mut self, event: &ScheduledEvent) -> Changes {
		let Some(guild) = self.guilds.get_mut(&event.guild_id) else {
			return Changes::default();
		};
		guild.upsert_scheduled_event(event);
		events_changed(event.guild_id)
	}

	pub(crate) fn scheduled_event_deleted(&mut self, event: &ScheduledEvent) -> Changes {
		let Some(guild) = self.guilds.get_mut(&event.guild_id) else {
			return Changes::default();
		};
		guild.remove_scheduled_event(event.id);
		events_changed(event.guild_id)
	}

	pub(crate) fn scheduled_event_interest(&mut self, change: &ScheduledEventUser, joined: bool) -> Changes {
		let mine = Some(change.user_id) == self.current_user_id();
		let Some(guild) = self.guilds.get_mut(&change.guild_id) else {
			return Changes::default();
		};
		if mine {
			if joined {
				guild.interested_events.insert(change.guild_scheduled_event_id);
			} else {
				guild.interested_events.remove(&change.guild_scheduled_event_id);
			}
		}
		if let Some(event) = guild
			.scheduled_events
			.iter_mut()
			.find(|event| event.id == change.guild_scheduled_event_id)
		{
			let count = event.user_count.unwrap_or(0);
			event.user_count = Some(if joined { count + 1 } else { (count - 1).max(0) });
		}
		events_loaded(change.guild_id)
	}

	pub(crate) fn scheduled_events_loaded(
		&mut self,
		guild_id: Snowflake,
		events: &[ScheduledEvent],
		interested: Option<&[Snowflake]>,
		requested: Instant,
	) -> Changes {
		let Some(guild) = self.guilds.get_mut(&guild_id) else {
			return Changes::default();
		};
		let stale = guild.events_touched.is_some_and(|touched| touched > requested);
		if !stale {
			guild.set_scheduled_events(events);
		}
		if let Some(interested) = interested {
			if stale {
				guild.interested_events.extend(interested.iter().copied());
			} else {
				guild.interested_events = interested.iter().copied().collect();
			}
		}
		events_changed(guild_id)
	}
}
