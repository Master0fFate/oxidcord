use std::fmt;

use chrono::{DateTime, Utc};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;
const TIMESTAMP_SHIFT: u32 = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Snowflake(pub u64);

impl Snowflake {
	pub fn get(self) -> u64 {
		self.0
	}

	pub fn timestamp(self) -> DateTime<Utc> {
		let millis = (self.0 >> TIMESTAMP_SHIFT) as i64 + DISCORD_EPOCH_MS;
		DateTime::from_timestamp_millis(millis).unwrap_or(DateTime::UNIX_EPOCH)
	}
}

impl From<u64> for Snowflake {
	fn from(value: u64) -> Self {
		Self(value)
	}
}

impl From<Snowflake> for u64 {
	fn from(value: Snowflake) -> Self {
		value.0
	}
}

impl fmt::Display for Snowflake {
	fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		self.0.fmt(formatter)
	}
}

impl Serialize for Snowflake {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.collect_str(&self.0)
	}
}

struct SnowflakeVisitor;

impl Visitor<'_> for SnowflakeVisitor {
	type Value = Snowflake;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a snowflake as a string or an unsigned integer")
	}

	fn visit_u64<E: de::Error>(self, value: u64) -> Result<Snowflake, E> {
		Ok(Snowflake(value))
	}

	fn visit_i64<E: de::Error>(self, value: i64) -> Result<Snowflake, E> {
		u64::try_from(value).map(Snowflake).map_err(E::custom)
	}

	fn visit_str<E: de::Error>(self, value: &str) -> Result<Snowflake, E> {
		value.parse().map(Snowflake).map_err(E::custom)
	}
}

impl<'de> Deserialize<'de> for Snowflake {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_any(SnowflakeVisitor)
	}
}
