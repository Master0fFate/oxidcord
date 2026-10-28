use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use plugin_host::catalog::{Catalog, CatalogEntry, ThemeCatalog};
use plugin_host::{
	Grants, HookFlags, LatencyStats, Package, Permission, PluginManager, PluginState, THEME_FILE,
};
use store::Store;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PluginFlags {
	pub(crate) events: bool,
	pub(crate) timeline: bool,
	pub(crate) render: bool,
	pub(crate) filter: bool,
	pub(crate) send: bool,
	pub(crate) display: bool,
	pub(crate) logs: bool,
	pub(crate) intercept: bool,
	pub(crate) hooks: HookFlags,
}

impl PluginFlags {
	pub(crate) fn renders(self) -> bool {
		self.render || self.filter || self.intercept
	}
}

pub(crate) struct PluginHub {
	pub(crate) manager: Arc<PluginManager>,
	pub(crate) grants: Arc<Grants>,
	pub(crate) store: Arc<Store>,
	pub(crate) stats: Arc<LatencyStats>,
	themes: Mutex<Vec<PathBuf>>,
	catalog: Mutex<Option<Arc<Catalog>>>,
	catalog_error: Mutex<Option<String>>,
	theme_catalog: Mutex<Option<Arc<ThemeCatalog>>>,
	theme_catalog_error: Mutex<Option<String>>,
	last_error: Mutex<Option<String>>,
}

impl PluginHub {
	pub(crate) fn new(manager: Arc<PluginManager>, grants: Arc<Grants>, store: Arc<Store>) -> Self {
		Self {
			manager,
			grants,
			store,
			stats: Arc::new(LatencyStats::default()),
			themes: Mutex::new(Vec::new()),
			catalog: Mutex::new(None),
			catalog_error: Mutex::new(None),
			theme_catalog: Mutex::new(None),
			theme_catalog_error: Mutex::new(None),
			last_error: Mutex::new(None),
		}
	}

	pub(crate) fn flags(&self) -> PluginFlags {
		PluginFlags {
			events: self.manager.has_instances(),
			timeline: self.manager.has_timeline_events(),
			render: self.manager.has_active(Permission::MessageRender),
			filter: self.manager.has_active(Permission::MessageFilter),
			send: self.manager.has_active(Permission::MessageSend),
			display: self.manager.has_active(Permission::UserDisplay),
			logs: self.manager.has_message_log(),
			intercept: self.manager.has_intercept(),
			hooks: self.manager.hook_flags(),
		}
	}

	pub(crate) fn theme_files(&self) -> Vec<PathBuf> {
		self.themes.lock().clone()
	}

	pub(crate) fn catalog(&self) -> Option<Arc<Catalog>> {
		self.catalog.lock().clone()
	}

	pub(crate) fn set_catalog(&self, outcome: Result<Catalog>) {
		match outcome {
			Ok(catalog) => {
				*self.catalog.lock() = Some(Arc::new(catalog));
				*self.catalog_error.lock() = None;
			}
			Err(error) => *self.catalog_error.lock() = Some(format!("{error:#}")),
		}
	}

	pub(crate) fn catalog_error(&self) -> Option<String> {
		self.catalog_error.lock().clone()
	}

	pub(crate) fn theme_catalog(&self) -> Option<Arc<ThemeCatalog>> {
		self.theme_catalog.lock().clone()
	}

	pub(crate) fn set_theme_catalog(&self, outcome: Result<ThemeCatalog>) {
		match outcome {
			Ok(catalog) => {
				*self.theme_catalog.lock() = Some(Arc::new(catalog));
				*self.theme_catalog_error.lock() = None;
			}
			Err(error) => self.set_theme_catalog_error(error),
		}
	}

	pub(crate) fn set_theme_catalog_error(&self, error: anyhow::Error) {
		*self.theme_catalog_error.lock() = Some(format!("{error:#}"));
	}

	pub(crate) fn clear_theme_catalog_error(&self) {
		*self.theme_catalog_error.lock() = None;
	}

	pub(crate) fn theme_catalog_error(&self) -> Option<String> {
		self.theme_catalog_error.lock().clone()
	}

	pub(crate) fn last_error(&self) -> Option<String> {
		self.last_error.lock().clone()
	}

	pub(crate) fn record(&self, what: &str, outcome: Result<()>) {
		let mut last = self.last_error.lock();
		match outcome {
			Ok(()) => *last = None,
			Err(error) => {
				tracing::warn!(target: "plugin", "{what}: {error:#}");
				*last = Some(format!("{what}: {error:#}"));
			}
		}
	}

	pub(crate) fn load_all(&self, safe_mode: bool) {
		let plugins_dir = self.store.paths.plugins_dir();
		for issue in self.manager.load_all(&plugins_dir, safe_mode) {
			tracing::warn!(target: "plugin", dir = %issue.dir.display(), "{}", issue.error);
		}
		self.sync_themes();
	}

	pub(crate) fn install(&self, archive: &Path, expected: Option<&CatalogEntry>) -> Result<()> {
		let package = Package::open(archive)?;
		if let Some(entry) = expected {
			ensure!(
				package.manifest.id == entry.id && package.manifest.version == entry.version,
				"the archive does not match its catalog entry"
			);
		}
		let installed = package.install_into(&self.store.paths.plugins_dir())?;
		let outcome = self.manager.load_one(&installed.dir);
		self.sync_themes();
		outcome?;
		Ok(())
	}

	pub(crate) fn uninstall(&self, id: &str) -> Result<()> {
		let plugins_dir = self.store.paths.plugins_dir();
		let dir = self.manager.unload(id)?;
		if !dir.starts_with(&plugins_dir) {
			bail!("{id} is not installed under the plugins directory");
		}
		let removable = match dir.parent() {
			Some(parent) if parent != plugins_dir => parent,
			_ => dir.as_path(),
		};
		fs::remove_dir_all(removable).with_context(|| format!("could not remove {}", removable.display()))?;
		self.grants.revoke_all(id)?;
		self.sync_themes();
		Ok(())
	}

	pub(crate) fn grant(&self, id: &str, permissions: &[Permission], hosts: &[String]) -> Result<()> {
		let manifest = self.manager.manifest(id)?;
		let requested: BTreeSet<Permission> = manifest
			.permissions
			.iter()
			.map(|request| request.permission)
			.collect();
		let granted: BTreeSet<Permission> = permissions
			.iter()
			.copied()
			.filter(|permission| requested.contains(permission))
			.collect();
		let approve = |allowed: &[String]| -> BTreeSet<String> {
			hosts
				.iter()
				.filter(|host| allowed.contains(host))
				.cloned()
				.collect()
		};
		let network_hosts = approve(&manifest.network_hosts);
		let image_hosts = approve(&manifest.image_hosts);
		let url_schemes = approve(&manifest.url_schemes);
		self.grants.update(id, |grant| {
			grant.permissions = granted;
			grant.network_hosts = network_hosts;
			grant.image_hosts = image_hosts;
			grant.url_schemes = url_schemes;
		})?;
		self.manager.reload(id)?;
		self.sync_themes();
		Ok(())
	}

	pub(crate) fn sync_themes(&self) {
		let themes_dir = self.store.paths.themes_dir();
		let mut wanted = Vec::new();
		for (id, dir, state) in self.manager.dirs() {
			let source = dir.join(THEME_FILE);
			if !matches!(state, PluginState::Loaded | PluginState::Crashed) || !source.is_file() {
				continue;
			}
			let target = themes_dir.join(format!("{id}.json"));
			match copy_if_changed(&source, &target) {
				Ok(()) => wanted.push(target),
				Err(error) => {
					tracing::warn!(target: "plugin", plugin = %id, "theme was not installed: {error:#}")
				}
			}
		}
		let mut themes = self.themes.lock();
		for stale in themes.iter().filter(|theme| !wanted.contains(theme)) {
			let _ = fs::remove_file(stale);
		}
		*themes = wanted;
	}
}

fn copy_if_changed(source: &Path, target: &Path) -> Result<()> {
	let bytes = fs::read(source).with_context(|| format!("could not read {}", source.display()))?;
	if fs::read(target).is_ok_and(|existing| existing == bytes) {
		return Ok(());
	}
	fs::write(target, bytes).with_context(|| format!("could not write {}", target.display()))
}
