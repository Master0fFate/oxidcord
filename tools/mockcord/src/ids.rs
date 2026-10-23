use protocol::DISCORD_EPOCH_MS;

const SEQUENCE_MASK: u64 = 0xFFF;
const TIMESTAMP_SHIFT: u32 = 22;
const ENTITY_EPOCH_MS: i64 = 1_640_995_200_000;

pub fn snowflake_at(unix_ms: i64, sequence: u64) -> u64 {
	(((unix_ms - DISCORD_EPOCH_MS).max(0) as u64) << TIMESTAMP_SHIFT) | (sequence & SEQUENCE_MASK)
}

pub struct EntityIds {
	next_ms: i64,
}

impl Default for EntityIds {
	fn default() -> Self {
		Self {
			next_ms: ENTITY_EPOCH_MS,
		}
	}
}

impl EntityIds {
	pub fn next(&mut self) -> u64 {
		let id = snowflake_at(self.next_ms, 0);
		self.next_ms += 1000;
		id
	}
}
