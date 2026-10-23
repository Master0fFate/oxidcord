use protocol::{RelationshipKind, StatusKind};

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

	fn relationship_rows(&self, kind: RelationshipKind) -> Vec<RelationshipRow> {
		self.relationships
			.values()
			.filter(|relationship| relationship.kind == kind)
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
				}
			})
			.collect()
	}
}
