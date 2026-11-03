use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::snowflake::Snowflake;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Token(String);

impl Token {
	pub fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}

	pub fn user_id(&self) -> Option<Snowflake> {
		let encoded = self.0.split('.').next()?.trim_end_matches('=');
		let decoded = URL_SAFE_NO_PAD
			.decode(encoded)
			.or_else(|_| STANDARD_NO_PAD.decode(encoded))
			.ok()?;
		std::str::from_utf8(&decoded).ok()?.parse().ok().map(Snowflake)
	}
}

impl fmt::Debug for Token {
	fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("Token(redacted)")
	}
}

impl Serialize for Token {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.0)
	}
}

impl<'de> Deserialize<'de> for Token {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		String::deserialize(deserializer).map(Self)
	}
}
