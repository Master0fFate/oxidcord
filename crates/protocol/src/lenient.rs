use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use chrono::{DateTime, Utc};
use serde::de::{self, DeserializeOwned, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

#[derive(Debug, Clone, PartialEq)]
pub struct LenientVec<T> {
	items: Vec<T>,
	dropped: usize,
}

impl<T> LenientVec<T> {
	pub fn dropped(&self) -> usize {
		self.dropped
	}

	pub fn into_vec(self) -> Vec<T> {
		self.items
	}
}

impl<T> Default for LenientVec<T> {
	fn default() -> Self {
		Self {
			items: Vec::new(),
			dropped: 0,
		}
	}
}

impl<T> From<Vec<T>> for LenientVec<T> {
	fn from(items: Vec<T>) -> Self {
		Self { items, dropped: 0 }
	}
}

impl<T> Deref for LenientVec<T> {
	type Target = Vec<T>;

	fn deref(&self) -> &Vec<T> {
		&self.items
	}
}

impl<T> DerefMut for LenientVec<T> {
	fn deref_mut(&mut self) -> &mut Vec<T> {
		&mut self.items
	}
}

impl<T> IntoIterator for LenientVec<T> {
	type Item = T;
	type IntoIter = std::vec::IntoIter<T>;

	fn into_iter(self) -> Self::IntoIter {
		self.items.into_iter()
	}
}

impl<'a, T> IntoIterator for &'a LenientVec<T> {
	type Item = &'a T;
	type IntoIter = std::slice::Iter<'a, T>;

	fn into_iter(self) -> Self::IntoIter {
		self.items.iter()
	}
}

impl<T: Serialize> Serialize for LenientVec<T> {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		self.items.serialize(serializer)
	}
}

fn collect_elements<'de, A, T>(mut seq: A) -> Result<LenientVec<T>, A::Error>
where
	A: SeqAccess<'de>,
	T: DeserializeOwned,
{
	let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
	let mut dropped = 0;
	while let Some(raw) = seq.next_element::<Box<RawValue>>()? {
		match serde_json::from_str::<T>(raw.get()) {
			Ok(item) => items.push(item),
			Err(_) => dropped += 1,
		}
	}
	Ok(LenientVec { items, dropped })
}

struct LenientVecVisitor<T>(PhantomData<T>);

impl<'de, T: DeserializeOwned> Visitor<'de> for LenientVecVisitor<T> {
	type Value = LenientVec<T>;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a sequence")
	}

	fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
		Ok(LenientVec::default())
	}

	fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
		Ok(LenientVec::default())
	}

	fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
		collect_elements(seq)
	}
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for LenientVec<T> {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_any(LenientVecVisitor(PhantomData))
	}
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntryContainer<T> {
	pub entries: LenientVec<T>,
	pub version: Option<u64>,
	pub partial: bool,
}

impl<T> Default for EntryContainer<T> {
	fn default() -> Self {
		Self {
			entries: LenientVec::default(),
			version: None,
			partial: false,
		}
	}
}

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ContainerField {
	Entries,
	Version,
	Partial,
	#[serde(other)]
	Other,
}

struct EntryContainerVisitor<T>(PhantomData<T>);

impl<'de, T: DeserializeOwned> Visitor<'de> for EntryContainerVisitor<T> {
	type Value = EntryContainer<T>;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a sequence or a map with entries")
	}

	fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
		Ok(EntryContainer::default())
	}

	fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
		Ok(EntryContainer::default())
	}

	fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
		Ok(EntryContainer {
			entries: collect_elements(seq)?,
			version: None,
			partial: false,
		})
	}

	fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
		let mut container = EntryContainer::default();
		while let Some(field) = map.next_key::<ContainerField>()? {
			match field {
				ContainerField::Entries => container.entries = map.next_value()?,
				ContainerField::Version => container.version = map.next_value()?,
				ContainerField::Partial => container.partial = map.next_value()?,
				ContainerField::Other => {
					map.next_value::<IgnoredAny>()?;
				}
			}
		}
		Ok(container)
	}
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for EntryContainer<T> {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_any(EntryContainerVisitor(PhantomData))
	}
}

struct IntVisitor;

impl Visitor<'_> for IntVisitor {
	type Value = i64;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("an integer")
	}

	fn visit_i64<E: de::Error>(self, value: i64) -> Result<i64, E> {
		Ok(value)
	}

	fn visit_u64<E: de::Error>(self, value: u64) -> Result<i64, E> {
		Ok(i64::try_from(value).unwrap_or(i64::MAX))
	}

	fn visit_f64<E: de::Error>(self, value: f64) -> Result<i64, E> {
		Ok(value as i64)
	}

	fn visit_str<E: de::Error>(self, value: &str) -> Result<i64, E> {
		value.parse().map_err(E::custom)
	}
}

pub fn int<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
	deserializer.deserialize_any(IntVisitor)
}

struct StringVisitor<T, F>(F, PhantomData<T>);

impl<T, F: FnOnce(&str) -> T> Visitor<'_> for StringVisitor<T, F> {
	type Value = T;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a string")
	}

	fn visit_str<E: de::Error>(self, value: &str) -> Result<T, E> {
		Ok((self.0)(value))
	}
}

pub fn string<'de, D, T, F>(deserializer: D, convert: F) -> Result<T, D::Error>
where
	D: Deserializer<'de>,
	F: FnOnce(&str) -> T,
{
	deserializer.deserialize_str(StringVisitor(convert, PhantomData))
}

struct BitsVisitor;

impl Visitor<'_> for BitsVisitor {
	type Value = u64;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("an integer or a decimal string")
	}

	fn visit_u64<E: de::Error>(self, value: u64) -> Result<u64, E> {
		Ok(value)
	}

	fn visit_i64<E: de::Error>(self, value: i64) -> Result<u64, E> {
		Ok(value as u64)
	}

	fn visit_f64<E: de::Error>(self, value: f64) -> Result<u64, E> {
		Ok(value as u64)
	}

	fn visit_str<E: de::Error>(self, value: &str) -> Result<u64, E> {
		Ok(value.parse().unwrap_or(0))
	}

	fn visit_unit<E: de::Error>(self) -> Result<u64, E> {
		Ok(0)
	}
}

pub fn bits<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
	deserializer.deserialize_any(BitsVisitor)
}

struct TimestampVisitor;

impl<'de> Visitor<'de> for TimestampVisitor {
	type Value = Option<DateTime<Utc>>;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("an RFC 3339 timestamp")
	}

	fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
		Ok(DateTime::parse_from_rfc3339(value)
			.ok()
			.map(|parsed| parsed.with_timezone(&Utc)))
	}

	fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
		Ok(None)
	}

	fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
		Ok(None)
	}

	fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
		deserializer.deserialize_any(self)
	}

	fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
		Ok(None)
	}

	fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
		Ok(None)
	}

	fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
		Ok(None)
	}

	fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
		Ok(None)
	}

	fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
		while seq.next_element::<IgnoredAny>()?.is_some() {}
		Ok(None)
	}

	fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
		while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
		Ok(None)
	}
}

pub fn timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error> {
	deserializer.deserialize_any(TimestampVisitor)
}

pub fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
	IgnoredAny::deserialize(deserializer).map(|_| true)
}

macro_rules! int_enum {
	(
		$(#[$meta:meta])*
		$vis:vis enum $name:ident {
			$first:ident = $first_value:literal
			$(, $variant:ident = $value:literal)*
			$(,)?
		}
	) => {
		$(#[$meta])*
		#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
		$vis enum $name {
			$first,
			$($variant,)*
			Unknown(i64),
		}

		impl $name {
			pub fn is_unknown(self) -> bool {
				matches!(self, Self::Unknown(_))
			}
		}

		impl Default for $name {
			fn default() -> Self {
				Self::$first
			}
		}

		impl From<i64> for $name {
			fn from(value: i64) -> Self {
				match value {
					$first_value => Self::$first,
					$($value => Self::$variant,)*
					other => Self::Unknown(other),
				}
			}
		}

		impl From<$name> for i64 {
			fn from(value: $name) -> Self {
				match value {
					$name::$first => $first_value,
					$($name::$variant => $value,)*
					$name::Unknown(other) => other,
				}
			}
		}

		impl ::serde::Serialize for $name {
			fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
				serializer.serialize_i64((*self).into())
			}
		}

		impl<'de> ::serde::Deserialize<'de> for $name {
			fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
				$crate::lenient::int(deserializer).map(Self::from)
			}
		}
	};
}

macro_rules! str_enum {
	(
		$(#[$meta:meta])*
		$vis:vis enum $name:ident {
			$first:ident = $first_value:literal
			$(, $variant:ident = $value:literal)*
			$(,)?
		}
	) => {
		$(#[$meta])*
		#[derive(Debug, Clone, PartialEq, Eq, Hash)]
		$vis enum $name {
			$first,
			$($variant,)*
			Unknown(String),
		}

		impl $name {
			pub fn as_str(&self) -> &str {
				match self {
					Self::$first => $first_value,
					$(Self::$variant => $value,)*
					Self::Unknown(other) => other,
				}
			}

			pub fn is_unknown(&self) -> bool {
				matches!(self, Self::Unknown(_))
			}
		}

		impl Default for $name {
			fn default() -> Self {
				Self::$first
			}
		}

		impl From<&str> for $name {
			fn from(value: &str) -> Self {
				match value {
					$first_value => Self::$first,
					$($value => Self::$variant,)*
					other => Self::Unknown(other.to_owned()),
				}
			}
		}

		impl ::serde::Serialize for $name {
			fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
				serializer.serialize_str(self.as_str())
			}
		}

		impl<'de> ::serde::Deserialize<'de> for $name {
			fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
				$crate::lenient::string(deserializer, |value| Self::from(value))
			}
		}
	};
}

macro_rules! flags_serde {
	($name:ident, number) => {
		$crate::lenient::flags_serde!(@common $name);

		impl ::serde::Serialize for $name {
			fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
				serializer.serialize_u64(self.bits())
			}
		}
	};
	($name:ident, string) => {
		$crate::lenient::flags_serde!(@common $name);

		impl ::serde::Serialize for $name {
			fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
				serializer.collect_str(&self.bits())
			}
		}
	};
	(@common $name:ident) => {
		impl Default for $name {
			fn default() -> Self {
				Self::empty()
			}
		}

		impl<'de> ::serde::Deserialize<'de> for $name {
			fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
				$crate::lenient::bits(deserializer).map(Self::from_bits_retain)
			}
		}
	};
}

pub(crate) use {flags_serde, int_enum, str_enum};
