use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Error, Result};
use crate::permission::Permission;

pub const CATALOG_VERSION: u32 = 1;

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

fn lenient_entries<'de, D: Deserializer<'de>>(
	deserializer: D,
) -> std::result::Result<Vec<CatalogEntry>, D::Error> {
	let raw = Vec::<serde_json::Value>::deserialize(deserializer)?;
	Ok(raw
		.into_iter()
		.filter_map(|value| serde_json::from_value(value).ok())
		.collect())
}

fn catalog_error(reason: &str) -> Error {
	Error::Catalog(reason.to_owned())
}

pub fn verify_catalog(bytes: &[u8], signature_base64: &[u8], key: &VerifyingKey) -> Result<Catalog> {
	let raw = STANDARD
		.decode(signature_base64.trim_ascii())
		.map_err(|_| catalog_error("signature is not base64"))?;
	let signature = Signature::from_slice(&raw).map_err(|_| catalog_error("signature is not 64 bytes"))?;
	key.verify_strict(bytes, &signature)
		.map_err(|_| catalog_error("signature does not match the catalog"))?;
	let catalog: Catalog = serde_json::from_slice(bytes)?;
	if catalog.version != CATALOG_VERSION {
		return Err(catalog_error("unsupported catalog version"));
	}
	Ok(catalog)
}
