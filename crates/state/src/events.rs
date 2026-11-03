use std::time::Instant;

use chrono::Utc;
use protocol::{ScheduledEvent, ScheduledEventStatus, ScheduledEventUser, Snowflake};

use crate::changes::Changes;
use crate::notifications::guild_muted;
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

	fn event_started_for_me(&self, event: &ScheduledEvent) -> bool {
		let Some(guild) = self.guilds.get(&event.guild_id) else {
			return false;
		};
		let was_scheduled = guild
			.scheduled_events
			.iter()
			.any(|known| known.id == event.id && known.status == ScheduledEventStatus::Scheduled);
		let settings = self.guild_settings(Some(event.guild_id));
		was_scheduled
			&& event.status == ScheduledEventStatus::Active
			&& guild.interested_events.contains(&event.id)
			&& event.creator_id != self.current_user_id()
			&& !settings.is_some_and(|settings| settings.mute_scheduled_events)
			&& !guild_muted(settings, Utc::now())
	}

	pub(crate) fn scheduled_event_upserted(&mut self, event: &ScheduledEvent) -> Changes {
		let started = self.event_started_for_me(event);
		let Some(guild) = self.guilds.get_mut(&event.guild_id) else {
			return Changes::default();
		};
		guild.upsert_scheduled_event(event);
		let mut changes = events_changed(event.guild_id);
		if started {
			self.note_event_notice(event.guild_id, event.id);
			changes.event_notice = true;
		}
		changes
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
