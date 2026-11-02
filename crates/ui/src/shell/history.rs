use protocol::Snowflake;

use super::friends::HomePage;

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Place {
	Channel {
		guild: Option<Snowflake>,
		channel: Snowflake,
	},
	Home(HomePage),
}

#[derive(Default)]
pub struct History {
	places: Vec<Place>,
	cursor: usize,
	travelling: Option<Place>,
}

impl History {
	pub fn visit(&mut self, place: Place) {
		if self.travelling.take() == Some(place) || self.places.get(self.cursor) == Some(&place) {
			return;
		}
		self.places.truncate(self.cursor + 1);
		self.places.push(place);
		if self.places.len() > HISTORY_LIMIT {
			self.places.remove(0);
		}
		self.cursor = self.places.len() - 1;
	}

	pub fn back(&mut self, reachable: impl Fn(&Place) -> bool) -> Option<Place> {
		let cursor = self.places[..self.cursor].iter().rposition(reachable)?;
		self.travel(cursor)
	}

	pub fn forward(&mut self, reachable: impl Fn(&Place) -> bool) -> Option<Place> {
		let skipped = self.places.get(self.cursor + 1..)?.iter().position(reachable)?;
		self.travel(self.cursor + 1 + skipped)
	}

	pub fn can_go_back(&self) -> bool {
		self.cursor > 0
	}

	pub fn can_go_forward(&self) -> bool {
		self.cursor + 1 < self.places.len()
	}

	fn travel(&mut self, cursor: usize) -> Option<Place> {
		let place = self.places.get(cursor).copied()?;
		self.cursor = cursor;
		self.travelling = Some(place);
		Some(place)
	}
}
