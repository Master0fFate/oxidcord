use protocol::Snowflake;

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Place {
	pub guild: Option<Snowflake>,
	pub channel: Snowflake,
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

	pub fn back(&mut self) -> Option<Place> {
		if !self.can_go_back() {
			return None;
		}
		self.cursor -= 1;
		self.travel()
	}

	pub fn forward(&mut self) -> Option<Place> {
		if !self.can_go_forward() {
			return None;
		}
		self.cursor += 1;
		self.travel()
	}

	pub fn can_go_back(&self) -> bool {
		self.cursor > 0
	}

	pub fn can_go_forward(&self) -> bool {
		self.cursor + 1 < self.places.len()
	}

	fn travel(&mut self) -> Option<Place> {
		let place = self.places.get(self.cursor).copied()?;
		self.travelling = Some(place);
		Some(place)
	}
}
