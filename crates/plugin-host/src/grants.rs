use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use store::Store;

use crate::error::Result;
use crate::manifest::Manifest;
use crate::permission::Permission;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginGrant {
	pub permissions: BTreeSet<Permission>,
	pub network_hosts: BTreeSet<String>,
	pub enabled: bool,
}

impl Default for PluginGrant {
	fn default() -> Self {
		Self {
			permissions: BTreeSet::new(),
			network_hosts: BTreeSet::new(),
			enabled: true,
		}
	}
}

pub struct Grants {
	store: Arc<Store>,
	cache: RwLock<HashMap<String, PluginGrant>>,
}

fn key(plugin_id: &str) -> String {
	format!("plugins.grants.{plugin_id}")
}

impl Grants {
	pub fn new(store: Arc<Store>) -> Self {
		Self {
			store,
			cache: RwLock::new(HashMap::new()),
		}
	}

	pub fn store(&self) -> &Arc<Store> {
		&self.store
	}

	pub fn get(&self, plugin_id: &str) -> PluginGrant {
		if let Some(grant) = self.cache.read().get(plugin_id) {
			return grant.clone();
		}
		let grant = self.load(plugin_id);
		self.cache
			.write()
			.entry(plugin_id.to_owned())
			.or_insert(grant)
			.clone()
	}

	fn load(&self, plugin_id: &str) -> PluginGrant {
		self.store
			.db
			.kv_get(&key(plugin_id))
			.ok()
			.flatten()
			.and_then(|bytes| serde_json::from_slice(&bytes).ok())
			.unwrap_or_default()
	}

	pub fn set(&self, plugin_id: &str, grant: PluginGrant) -> Result<()> {
		self.store
			.db
			.kv_set(&key(plugin_id), &serde_json::to_vec(&grant)?)?;
		self.cache.write().insert(plugin_id.to_owned(), grant);
		Ok(())
	}

	pub fn update(&self, plugin_id: &str, apply: impl FnOnce(&mut PluginGrant)) -> Result<()> {
		let mut grant = self.get(plugin_id);
		apply(&mut grant);
		self.set(plugin_id, grant)
	}

	pub fn grant_manifest(&self, manifest: &Manifest) -> Result<()> {
		self.update(&manifest.id, |grant| {
			grant
				.permissions
				.extend(manifest.permissions.iter().map(|request| request.permission));
			grant.network_hosts.extend(manifest.network_hosts.iter().cloned());
		})
	}

	pub fn revoke_all(&self, plugin_id: &str) -> Result<()> {
		self.update(plugin_id, |grant| {
			grant.permissions.clear();
			grant.network_hosts.clear();
		})
	}

	pub fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<()> {
		self.update(plugin_id, |grant| grant.enabled = enabled)
	}

	pub fn has(&self, plugin_id: &str, permission: Permission) -> bool {
		if let Some(grant) = self.cache.read().get(plugin_id) {
			return grant.permissions.contains(&permission);
		}
		self.get(plugin_id).permissions.contains(&permission)
	}

	pub fn allows_host(&self, plugin_id: &str, host: &str) -> bool {
		self.get(plugin_id).network_hosts.contains(host)
	}

	pub fn requires_reprompt(&self, manifest: &Manifest) -> bool {
		let grant = self.get(&manifest.id);
		manifest
			.permissions
			.iter()
			.any(|request| !grant.permissions.contains(&request.permission))
			|| manifest
				.network_hosts
				.iter()
				.any(|host| !grant.network_hosts.contains(host))
	}
}
