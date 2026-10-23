use std::collections::BTreeMap;
use std::sync::Arc;

use store::Store;

use crate::api::HostError;

pub const STORAGE_QUOTA_BYTES: usize = 5 * 1024 * 1024;
const MAX_KEYS: usize = 1024;
const MAX_KEY_BYTES: usize = 256;

pub struct PluginStorage {
	store: Arc<Store>,
	plugin_id: String,
	index: Option<BTreeMap<String, usize>>,
}

fn invalid(reason: &str) -> HostError {
	HostError::Invalid(reason.to_owned())
}

impl PluginStorage {
	pub fn new(store: Arc<Store>, plugin_id: &str) -> Self {
		Self {
			store,
			plugin_id: plugin_id.to_owned(),
			index: None,
		}
	}

	fn data_key(&self, key: &str) -> String {
		format!("plugins.data.{}.{key}", self.plugin_id)
	}

	fn index_key(&self) -> String {
		format!("plugins.index.{}", self.plugin_id)
	}

	fn index(&mut self) -> Result<&mut BTreeMap<String, usize>, HostError> {
		if self.index.is_none() {
			let loaded = self
				.store
				.db
				.kv_get(&self.index_key())
				.map_err(|_| HostError::Unavailable)?
				.and_then(|bytes| serde_json::from_slice(&bytes).ok())
				.unwrap_or_default();
			self.index = Some(loaded);
		}
		self.index.as_mut().ok_or(HostError::Unavailable)
	}

	fn persist_index(&self) -> Result<(), HostError> {
		let index = self.index.as_ref().ok_or(HostError::Unavailable)?;
		let bytes = serde_json::to_vec(index).map_err(|_| HostError::Unavailable)?;
		self.store
			.db
			.kv_set(&self.index_key(), &bytes)
			.map_err(|_| HostError::Unavailable)
	}

	pub fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, HostError> {
		validate_key(key)?;
		if !self.index()?.contains_key(key) {
			return Ok(None);
		}
		self.store
			.db
			.kv_get(&self.data_key(key))
			.map_err(|_| HostError::Unavailable)
	}

	pub fn set(&mut self, key: &str, value: &[u8]) -> Result<(), HostError> {
		validate_key(key)?;
		let entry_size = key.len() + value.len();
		let index = self.index()?;
		let previous = index.get(key).map_or(0, |size| key.len() + size);
		let used: usize = index.iter().map(|(name, size)| name.len() + size).sum();
		if !index.contains_key(key) && index.len() >= MAX_KEYS {
			return Err(invalid("too many storage keys"));
		}
		if used - previous + entry_size > STORAGE_QUOTA_BYTES {
			return Err(invalid("storage quota exceeded"));
		}
		self.store
			.db
			.kv_set(&self.data_key(key), value)
			.map_err(|_| HostError::Unavailable)?;
		self.index()?.insert(key.to_owned(), value.len());
		self.persist_index()
	}

	pub fn delete(&mut self, key: &str) -> Result<(), HostError> {
		validate_key(key)?;
		if self.index()?.remove(key).is_none() {
			return Ok(());
		}
		self.store
			.db
			.kv_set(&self.data_key(key), &[])
			.map_err(|_| HostError::Unavailable)?;
		self.persist_index()
	}

	pub fn list(&mut self) -> Result<Vec<String>, HostError> {
		Ok(self.index()?.keys().cloned().collect())
	}
}

fn validate_key(key: &str) -> Result<(), HostError> {
	if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
		return Err(invalid("invalid storage key"));
	}
	Ok(())
}
