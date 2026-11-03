use std::collections::HashMap;

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

	pub fn pending_incoming_count(&self) -> usize {
		self.relationships
			.values()
			.filter(|relationship| {
				relationship.kind == RelationshipKind::IncomingRequest && !relationship.is_spam_request
			})
			.count()
	}

	pub fn is_ignored(&self, user_id: Snowflake) -> bool {
		self.relationships
			.get(&user_id)
			.is_some_and(|relationship| relationship.user_ignored)
	}

	fn relationship_rows(&self, kind: RelationshipKind) -> Vec<RelationshipRow> {
		self.rows_where(|relationship| relationship.kind == kind)
	}

	fn rows_where(&self, keep: impl Fn(&Relationship) -> bool) -> Vec<RelationshipRow> {
		self.relationships
			.values()
			.filter(|relationship| keep(relationship))
			.map(|relationship| {
				let user = self.users.get(&relationship.id);
				RelationshipRow {
					user_id: relationship.id,
					username: user.map(|user| user.username.clone()).unwrap_or_default(),
					display: self.display_name(None, relationship.id),
					avatar: user.and_then(|user| user.avatar.clone()),
					nickname: relationship.nickname.clone(),
					status: self
						.presences
						.get(&relationship.id)
						.map_or(StatusKind::Offline, |presence| presence.status.clone()),
					activity: self.activity_summary(relationship.id),
					since: relationship.since,
					ignored: relationship.user_ignored,
					spam: relationship.is_spam_request,
					stranger: relationship.stranger_request,
				}
			})
			.collect()
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
