use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::RwLock;

use crate::api::model::{MenuItem, MountOptions, UiContext, ViewTree};

pub const STATIC_POINTS: [&str; 9] = [
	"title-bar",
	"status-bar",
	"composer-toolbar",
	"message-action",
	"sidebar-panel",
	"right-panel",
	"app-nav",
	"main-view",
	"settings-page",
];

pub const CONTEXT_POINTS: [&str; 4] = [
	"channel-header",
	"composer-above",
	"user-popout-section",
	"member-list-section",
];

pub const MENUS: [&str; 5] = [
	"message-context-menu",
	"user-context-menu",
	"channel-context-menu",
	"server-context-menu",
	"link-context-menu",
];

const MAX_MOUNTS_PER_PLUGIN: usize = 32;
const MAX_CACHED_VIEWS: usize = 512;
const MAX_QUEUED_OVERLAYS: usize = 8;

pub fn static_point(name: &str) -> Option<&'static str> {
	STATIC_POINTS.iter().copied().find(|point| *point == name)
}

pub fn context_point(name: &str) -> Option<&'static str> {
	CONTEXT_POINTS.iter().copied().find(|point| *point == name)
}

pub fn menu(name: &str) -> Option<&'static str> {
	MENUS.iter().copied().find(|menu| *menu == name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ViewContext {
	pub guild_id: Option<u64>,
	pub channel_id: Option<u64>,
	pub message_id: Option<u64>,
	pub user_id: Option<u64>,
}

impl ViewContext {
	pub fn to_api(self, url: Option<String>) -> UiContext {
		UiContext {
			guild_id: self.guild_id,
			channel_id: self.channel_id,
			message_id: self.message_id,
			user_id: self.user_id,
			url,
		}
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mount {
	pub plugin_id: Arc<str>,
	pub key: String,
	pub tree: ViewTree,
	pub options: MountOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
	pub plugin_id: Arc<str>,
	pub options: MountOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuEntry {
	pub plugin_id: Arc<str>,
	pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
	Toast {
		plugin_id: Arc<str>,
		tree: ViewTree,
		ttl: Duration,
	},
	Modal {
		plugin_id: Arc<str>,
		title: Option<String>,
		tree: ViewTree,
	},
	Close {
		plugin_id: Arc<str>,
	},
	Insert {
		plugin_id: Arc<str>,
		text: String,
	},
}

#[derive(Debug, Clone, PartialEq)]
pub enum Contextual {
	Pending,
	Ready(Option<ViewTree>),
}

type ViewKey = (Arc<str>, &'static str, ViewContext);

#[derive(Default)]
struct Inner {
	mounts: HashMap<&'static str, Vec<Mount>>,
	providers: HashMap<&'static str, Vec<Provider>>,
	views: HashMap<ViewKey, Contextual>,
	menus: HashMap<&'static str, Vec<MenuEntry>>,
	overlays: VecDeque<Overlay>,
}

#[derive(Default)]
pub struct Mounts {
	inner: RwLock<Inner>,
	revision: AtomicU64,
}

pub enum MountError {
	Limit,
	Missing,
}

impl Mounts {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	fn bump(&self) {
		self.revision.fetch_add(1, Ordering::AcqRel);
	}

	pub fn mounted(&self, point: &str) -> Vec<Mount> {
		self.inner.read().mounts.get(point).cloned().unwrap_or_default()
	}

	pub fn mount(&self, point: &'static str, mount: Mount) -> Result<(), MountError> {
		let mut inner = self.inner.write();
		let owned = inner
			.mounts
			.values()
			.flatten()
			.filter(|existing| existing.plugin_id == mount.plugin_id)
			.count();
		let list = inner.mounts.entry(point).or_default();
		match list
			.iter_mut()
			.find(|existing| existing.plugin_id == mount.plugin_id && existing.key == mount.key)
		{
			Some(existing) if *existing == mount => return Ok(()),
			Some(existing) => *existing = mount,
			None if owned >= MAX_MOUNTS_PER_PLUGIN => return Err(MountError::Limit),
			None => {
				list.push(mount);
				list.sort_by(|left, right| {
					(left.options.order, &left.plugin_id, &left.key).cmp(&(
						right.options.order,
						&right.plugin_id,
						&right.key,
					))
				});
			}
		}
		drop(inner);
		self.bump();
		Ok(())
	}

	pub fn update(
		&self,
		point: &'static str,
		plugin_id: &str,
		key: &str,
		tree: ViewTree,
	) -> Result<(), MountError> {
		let mut inner = self.inner.write();
		let existing = inner
			.mounts
			.get_mut(point)
			.and_then(|list| {
				list.iter_mut()
					.find(|mount| &*mount.plugin_id == plugin_id && mount.key == key)
			})
			.ok_or(MountError::Missing)?;
		if existing.tree == tree {
			return Ok(());
		}
		existing.tree = tree;
		drop(inner);
		self.bump();
		Ok(())
	}

	pub fn unmount(&self, point: &'static str, plugin_id: &str, key: &str) {
		let mut inner = self.inner.write();
		let Some(list) = inner.mounts.get_mut(point) else {
			return;
		};
		let before = list.len();
		list.retain(|mount| !(&*mount.plugin_id == plugin_id && mount.key == key));
		let changed = list.len() != before;
		drop(inner);
		if changed {
			self.bump();
		}
	}

	pub fn provide(&self, point: &'static str, provider: Provider) {
		let mut inner = self.inner.write();
		let list = inner.providers.entry(point).or_default();
		match list
			.iter_mut()
			.find(|existing| existing.plugin_id == provider.plugin_id)
		{
			Some(existing) => *existing = provider,
			None => list.push(provider),
		}
		list.sort_by(|left, right| {
			(left.options.order, &left.plugin_id).cmp(&(right.options.order, &right.plugin_id))
		});
		inner.views.retain(|(_, cached, _), _| *cached != point);
		drop(inner);
		self.bump();
	}

	pub fn providers(&self, point: &str) -> Vec<Provider> {
		self.inner
			.read()
			.providers
			.get(point)
			.cloned()
			.unwrap_or_default()
	}

	pub fn contextual(
		&self,
		point: &'static str,
		context: ViewContext,
	) -> Vec<(Provider, Option<Contextual>)> {
		let inner = self.inner.read();
		inner
			.providers
			.get(point)
			.into_iter()
			.flatten()
			.map(|provider| {
				let cached = inner
					.views
					.get(&(Arc::clone(&provider.plugin_id), point, context))
					.cloned();
				(provider.clone(), cached)
			})
			.collect()
	}

	pub fn request(&self, point: &'static str, context: ViewContext) -> Vec<Arc<str>> {
		let mut inner = self.inner.write();
		let wanted: Vec<Arc<str>> = inner
			.providers
			.get(point)
			.into_iter()
			.flatten()
			.map(|provider| Arc::clone(&provider.plugin_id))
			.filter(|plugin_id| !inner.views.contains_key(&(Arc::clone(plugin_id), point, context)))
			.collect();
		if inner.views.len() + wanted.len() > MAX_CACHED_VIEWS {
			inner.views.clear();
		}
		for plugin_id in &wanted {
			inner
				.views
				.insert((Arc::clone(plugin_id), point, context), Contextual::Pending);
		}
		wanted
	}

	pub fn store_view(
		&self,
		plugin_id: &Arc<str>,
		point: &'static str,
		context: ViewContext,
		tree: Option<ViewTree>,
	) {
		self.inner
			.write()
			.views
			.insert((Arc::clone(plugin_id), point, context), Contextual::Ready(tree));
		self.bump();
	}

	pub fn invalidate(&self, plugin_id: &str, point: &'static str) {
		let mut inner = self.inner.write();
		let before = inner.views.len();
		inner
			.views
			.retain(|(owner, cached, _), _| !(&**owner == plugin_id && *cached == point));
		let changed = inner.views.len() != before;
		drop(inner);
		if changed {
			self.bump();
		}
	}

	pub fn set_menu(&self, menu: &'static str, entry: MenuEntry) {
		let mut inner = self.inner.write();
		let list = inner.menus.entry(menu).or_default();
		list.retain(|existing| existing.plugin_id != entry.plugin_id);
		if !entry.items.is_empty() {
			list.push(entry);
			list.sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));
		}
		drop(inner);
		self.bump();
	}

	pub fn menu_items(&self, menu: &str) -> Vec<MenuEntry> {
		self.inner.read().menus.get(menu).cloned().unwrap_or_default()
	}

	pub fn push_overlay(&self, overlay: Overlay) {
		let mut inner = self.inner.write();
		if inner.overlays.len() >= MAX_QUEUED_OVERLAYS {
			inner.overlays.pop_front();
		}
		inner.overlays.push_back(overlay);
		drop(inner);
		self.bump();
	}

	pub fn take_overlays(&self) -> Vec<Overlay> {
		let mut inner = self.inner.write();
		if inner.overlays.is_empty() {
			return Vec::new();
		}
		inner.overlays.drain(..).collect()
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut inner = self.inner.write();
		for list in inner.mounts.values_mut() {
			list.retain(|mount| &*mount.plugin_id != plugin_id);
		}
		for list in inner.providers.values_mut() {
			list.retain(|provider| &*provider.plugin_id != plugin_id);
		}
		for list in inner.menus.values_mut() {
			list.retain(|entry| &*entry.plugin_id != plugin_id);
		}
		inner.views.retain(|(owner, _, _), _| &**owner != plugin_id);
		inner.overlays.retain(|overlay| match overlay {
			Overlay::Toast { plugin_id: owner, .. }
			| Overlay::Modal { plugin_id: owner, .. }
			| Overlay::Close { plugin_id: owner }
			| Overlay::Insert { plugin_id: owner, .. } => &**owner != plugin_id,
		});
		drop(inner);
		self.bump();
	}
}
