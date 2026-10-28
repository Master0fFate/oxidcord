use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use store::Store;

use crate::convert::ImagePolicy;
use crate::error::Result;
use crate::manifest::Manifest;
use crate::permission::Permission;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginGrant {
	pub permissions: BTreeSet<Permission>,
	pub network_hosts: BTreeSet<String>,
	pub image_hosts: BTreeSet<String>,
	pub enabled: bool,
}

impl Default for PluginGrant {
	fn default() -> Self {
		Self {
			permissions: BTreeSet::new(),
			network_hosts: BTreeSet::new(),
			image_hosts: BTreeSet::new(),
			enabled: true,
		}
	}
}

#[derive(Clone)]
struct Resolved {
	grant: Arc<PluginGrant>,
	images: Arc<ImagePolicy>,
}

pub struct Grants {
	store: Arc<Store>,
	cache: RwLock<HashMap<String, Resolved>>,
	revision: AtomicU64,
}

fn key(plugin_id: &str) -> String {
	format!("plugins.grants.{plugin_id}")
}

impl Resolved {
	fn new(grant: PluginGrant) -> Self {
		let images = Arc::new(ImagePolicy::new(false, grant.image_hosts.clone()));
		Self {
			grant: Arc::new(grant),
			images,
		}
	}
}

impl Grants {
	pub fn new(store: Arc<Store>) -> Self {
		Self {
			store,
			cache: RwLock::new(HashMap::new()),
			revision: AtomicU64::new(0),
		}
	}

	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn store(&self) -> &Arc<Store> {
		&self.store
	}

	fn resolved(&self, plugin_id: &str) -> Resolved {
		if let Some(resolved) = self.cache.read().get(plugin_id) {
			return resolved.clone();
		}
		let resolved = Resolved::new(self.load(plugin_id));
		self.cache
			.write()
			.entry(plugin_id.to_owned())
			.or_insert(resolved)
			.clone()
	}

	fn read<T>(&self, plugin_id: &str, read: impl FnOnce(&PluginGrant) -> T) -> T {
		if let Some(resolved) = self.cache.read().get(plugin_id) {
			return read(&resolved.grant);
		}
		read(&self.resolved(plugin_id).grant)
	}

	pub fn get(&self, plugin_id: &str) -> Arc<PluginGrant> {
		self.resolved(plugin_id).grant
	}

	pub fn image_policy(&self, plugin_id: &str) -> Arc<ImagePolicy> {
		self.resolved(plugin_id).images
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
		self.cache
			.write()
			.insert(plugin_id.to_owned(), Resolved::new(grant));
		self.revision.fetch_add(1, Ordering::AcqRel);
		Ok(())
	}

	pub fn update(&self, plugin_id: &str, apply: impl FnOnce(&mut PluginGrant)) -> Result<()> {
		let mut grant = PluginGrant::clone(&self.get(plugin_id));
		apply(&mut grant);
		self.set(plugin_id, grant)
	}

	pub fn grant_manifest(&self, manifest: &Manifest) -> Result<()> {
		self.update(&manifest.id, |grant| {
			grant
				.permissions
				.extend(manifest.permissions.iter().map(|request| request.permission));
			grant.network_hosts.extend(manifest.network_hosts.iter().cloned());
			grant.image_hosts.extend(manifest.image_hosts.iter().cloned());
		})
	}

	pub fn revoke_all(&self, plugin_id: &str) -> Result<()> {
		self.update(plugin_id, |grant| {
			grant.permissions.clear();
			grant.network_hosts.clear();
			grant.image_hosts.clear();
		})
	}

	pub fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<()> {
		self.update(plugin_id, |grant| grant.enabled = enabled)
	}

	pub fn has(&self, plugin_id: &str, permission: Permission) -> bool {
		self.read(plugin_id, |grant| grant.permissions.contains(&permission))
	}

	pub fn allows_host(&self, plugin_id: &str, host: &str) -> bool {
		self.read(plugin_id, |grant| grant.network_hosts.contains(host))
	}

	pub fn allows_image_host(&self, plugin_id: &str, host: &str) -> bool {
		self.read(plugin_id, |grant| grant.image_hosts.contains(host))
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
			|| manifest
				.image_hosts
				.iter()
				.any(|host| !grant.image_hosts.contains(host))
	}
}
