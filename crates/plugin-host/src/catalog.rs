use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Error, Result};
use crate::permission::Permission;

pub const CATALOG_VERSION: u32 = 1;
const MAX_THEME_ID_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
	pub id: String,
	pub name: String,
	pub version: Version,
	pub description: String,
	pub permissions: Vec<Permission>,
	pub url: String,
	pub sha256: String,
	pub length: u64,
	pub reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
	pub version: u32,
	pub generated_at: String,
	#[serde(deserialize_with = "lenient_entries")]
	pub plugins: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeEntry {
	pub id: String,
	pub name: String,
	pub version: String,
	#[serde(default)]
	pub description: String,
	#[serde(default)]
	pub author: String,
	pub url: String,
	pub sha256: String,
	pub length: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeCatalog {
	pub version: u32,
	pub generated_at: String,
	#[serde(deserialize_with = "lenient_theme_entries")]
	pub themes: Vec<ThemeEntry>,
}

pub fn is_valid_theme_id(id: &str) -> bool {
	(1..=MAX_THEME_ID_LEN).contains(&id.len())
		&& id
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn lenient_theme_entries<'de, D: Deserializer<'de>>(
	deserializer: D,
) -> std::result::Result<Vec<ThemeEntry>, D::Error> {
	let mut entries: Vec<ThemeEntry> = lenient_entries(deserializer)?;
	entries.retain(|entry| is_valid_theme_id(&entry.id));
	Ok(entries)
}

fn lenient_entries<'de, T: DeserializeOwned, D: Deserializer<'de>>(
	deserializer: D,
) -> std::result::Result<Vec<T>, D::Error> {
	let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
	Ok(raw
		.into_iter()
		.filter_map(|value| serde_json::from_value(value).ok())
		.collect())
}

fn catalog_error(reason: &str) -> Error {
	Error::Catalog(reason.to_owned())
}

fn verify_signature(bytes: &[u8], signature_base64: &[u8], key: &VerifyingKey) -> Result<()> {
	let raw = STANDARD
		.decode(signature_base64.trim_ascii())
		.map_err(|_| catalog_error("signature is not base64"))?;
	let signature = Signature::from_slice(&raw).map_err(|_| catalog_error("signature is not 64 bytes"))?;
	key.verify_strict(bytes, &signature)
		.map_err(|_| catalog_error("signature does not match the catalog"))
}

pub fn verify_catalog(bytes: &[u8], signature_base64: &[u8], key: &VerifyingKey) -> Result<Catalog> {
	verify_signature(bytes, signature_base64, key)?;
	let catalog: Catalog = serde_json::from_slice(bytes)?;
	if catalog.version != CATALOG_VERSION {
		return Err(catalog_error("unsupported catalog version"));
	}
	Ok(catalog)
}

pub fn verify_theme_catalog(
	bytes: &[u8],
	signature_base64: &[u8],
	key: &VerifyingKey,
) -> Result<ThemeCatalog> {
	verify_signature(bytes, signature_base64, key)?;
	let catalog: ThemeCatalog = serde_json::from_slice(bytes)?;
	if catalog.version != CATALOG_VERSION {
		return Err(catalog_error("unsupported catalog version"));
	}
	Ok(catalog)
}
