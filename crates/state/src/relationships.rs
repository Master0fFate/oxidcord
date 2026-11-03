use std::collections::HashMap;

use chrono::{DateTime, Utc};
use protocol::{Relationship, RelationshipKind, Snowflake, StatusKind};

use crate::state::State;
use crate::views::RelationshipRow;

fn online_rank(status: &StatusKind) -> u8 {
	u8::from(matches!(status, StatusKind::Offline | StatusKind::Invisible))
}

impl State {
	pub fn friends(&self) -> Vec<RelationshipRow> {
		let mut rows = self.relationship_rows(RelationshipKind::Friend);
		rows.sort_by_cached_key(|row| (online_rank(&row.status), row.display.to_lowercase()));
		rows
	}

	pub fn pending_incoming(&self) -> Vec<RelationshipRow> {
		self.relationship_rows(RelationshipKind::IncomingRequest)
	}

	pub fn pending_outgoing(&self) -> Vec<RelationshipRow> {
		self.relationship_rows(RelationshipKind::OutgoingRequest)
	}

	pub fn blocked(&self) -> Vec<RelationshipRow> {
		self.relationship_rows(RelationshipKind::Blocked)
	}

	pub fn ignored(&self) -> Vec<RelationshipRow> {
		self.rows_where(|relationship| relationship.user_ignored)
	}

	pub fn pending_incoming_count(&self) -> u32 {
		let pending = self
			.relationships
			.values()
			.filter(|relationship| {
				relationship.kind == RelationshipKind::IncomingRequest && !relationship.is_spam_request
			})
			.count();
		u32::try_from(pending).unwrap_or(u32::MAX)
	}

	pub fn is_ignored(&self, user_id: Snowflake) -> bool {
		self.relationships
			.get(&user_id)
			.is_some_and(|relationship| relationship.user_ignored)
	}

	pub fn is_blocked(&self, user_id: Snowflake) -> bool {
		self.relationships
			.get(&user_id)
			.is_some_and(|relationship| relationship.kind == RelationshipKind::Blocked)
	}

	pub fn hides_messages_of(&self, user_id: Snowflake) -> bool {
		self.relationships.get(&user_id).is_some_and(|relationship| {
			relationship.kind == RelationshipKind::Blocked || relationship.user_ignored
		})
	}

	fn relationship_rows(&self, kind: RelationshipKind) -> Vec<RelationshipRow> {
		self.rows_where(|relationship| relationship.kind == kind)
	}

	fn rows_where(&self, keep: impl Fn(&Relationship) -> bool) -> Vec<RelationshipRow> {
		self.relationships
			.values()
			.filter(|relationship| keep(relationship))
			.map(|relationship| RelationshipRow {
				ignored: relationship.user_ignored,
				spam: relationship.is_spam_request,
				stranger: relationship.stranger_request,
				..self.person_row(relationship.id, relationship.nickname.clone(), relationship.since)
			})
			.collect()
	}

	pub(crate) fn person_row(
		&self,
		user_id: Snowflake,
		nickname: Option<String>,
		since: Option<DateTime<Utc>>,
	) -> RelationshipRow {
		let user = self.users.get(&user_id);
		RelationshipRow {
			user_id,
			username: user.map(|user| user.username.clone()).unwrap_or_default(),
			display: self.display_name(None, user_id),
			avatar: user.and_then(|user| user.avatar.clone()),
			nickname,
			status: self
				.presences
				.get(&user_id)
				.map_or(StatusKind::Offline, |presence| presence.status.clone()),
			activity: self.activity_summary(user_id),
			since,
			ignored: false,
			spam: false,
			stranger: false,
		}
	}
}

const IMPLICIT_MUTUAL_GUILDS: u8 = 2;

impl State {
	pub fn people_fingerprint(&self) -> (usize, usize, usize) {
		let members = self.guilds.values().map(|guild| guild.members.len()).sum();
		(self.guilds.len(), members, self.relationships.len())
	}

	pub fn member_ids(&self) -> Vec<Snowflake> {
		self.guilds
			.values()
			.flat_map(|guild| guild.members.keys().copied())
			.collect()
	}

	pub fn implicit_among(&self, shared: Vec<Snowflake>) -> Vec<Snowflake> {
		let me = self.current_user_id();
		let mut found: Vec<Snowflake> = shared
			.into_iter()
			.filter(|user_id| {
				Some(*user_id) != me
					&& !self.relationships.contains_key(user_id)
					&& self
						.users
						.get(user_id)
						.is_some_and(|user| !user.bot.unwrap_or(false))
			})
			.collect();
		found.sort_unstable();
		found
	}
}

pub fn shared_members(member_ids: Vec<Snowflake>) -> Vec<Snowflake> {
	let mut shared: HashMap<Snowflake, u8> = HashMap::new();
	for user_id in member_ids {
		let count = shared.entry(user_id).or_default();
		*count = count.saturating_add(1);
	}
	shared
		.into_iter()
		.filter(|(_, count)| *count >= IMPLICIT_MUTUAL_GUILDS)
		.map(|(user_id, _)| user_id)
		.collect()
}
