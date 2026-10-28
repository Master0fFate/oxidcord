use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
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
	pub tree: Arc<ViewTree>,
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
	pub items: Arc<[MenuItem]>,
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
	Ready(Option<Arc<ViewTree>>),
}

pub struct Requested {
	pub plugin_id: Arc<str>,
	pub stamp: u64,
}

type ViewKey = (Arc<str>, &'static str, ViewContext);
type StampKey = (Arc<str>, &'static str);

static NO_MOUNTS: LazyLock<Arc<[Mount]>> = LazyLock::new(|| Arc::from([]));
static NO_PROVIDERS: LazyLock<Arc<[Provider]>> = LazyLock::new(|| Arc::from([]));
static NO_MENUS: LazyLock<Arc<[MenuEntry]>> = LazyLock::new(|| Arc::from([]));

#[derive(Default)]
struct Inner {
	mounts: HashMap<&'static str, Arc<[Mount]>>,
	counts: HashMap<Arc<str>, usize>,
	providers: HashMap<&'static str, Arc<[Provider]>>,
	views: HashMap<ViewKey, Contextual>,
	stamps: HashMap<StampKey, u64>,
	next_stamp: u64,
	menus: HashMap<&'static str, Arc<[MenuEntry]>>,
	overlays: VecDeque<Overlay>,
}

impl Inner {
	fn stamp_of(&self, plugin_id: &Arc<str>, point: &'static str) -> u64 {
		self.stamps
			.get(&(Arc::clone(plugin_id), point))
			.copied()
			.unwrap_or(0)
	}

	fn restamp(&mut self, plugin_id: &Arc<str>, point: &'static str) {
		self.next_stamp += 1;
		self.stamps
			.insert((Arc::clone(plugin_id), point), self.next_stamp);
	}

	fn release(&mut self, plugin_id: &str, count: usize) {
		if let Some(owned) = self.counts.get_mut(plugin_id) {
			*owned = owned.saturating_sub(count);
			if *owned == 0 {
				self.counts.remove(plugin_id);
			}
		}
	}
}

fn edit<T: Clone>(list: &mut Arc<[T]>, change: impl FnOnce(&mut Vec<T>)) {
	let mut items = list.to_vec();
	change(&mut items);
	*list = items.into();
}

#[derive(Default)]
pub struct Mounts {
	inner: RwLock<Inner>,
	surfaces: AtomicU64,
	overlays: AtomicU64,
}

pub enum MountError {
	Limit,
	Missing,
}

impl Mounts {
	pub fn surfaces_revision(&self) -> u64 {
		self.surfaces.load(Ordering::Acquire)
	}

	pub fn overlays_revision(&self) -> u64 {
		self.overlays.load(Ordering::Acquire)
	}

	fn bump_surfaces(&self) {
		self.surfaces.fetch_add(1, Ordering::AcqRel);
	}

	fn bump_overlays(&self) {
		self.overlays.fetch_add(1, Ordering::AcqRel);
	}

	pub fn mounted(&self, point: &str) -> Arc<[Mount]> {
		self.inner
			.read()
			.mounts
			.get(point)
			.map_or_else(|| Arc::clone(&NO_MOUNTS), Arc::clone)
	}

	pub fn mount(&self, point: &'static str, mount: Mount) -> Result<(), MountError> {
		let mut guard = self.inner.write();
		let Inner { mounts, counts, .. } = &mut *guard;
		let list = mounts.entry(point).or_insert_with(|| Arc::clone(&NO_MOUNTS));
		let existing = list
			.iter()
			.position(|existing| existing.plugin_id == mount.plugin_id && existing.key == mount.key);
		match existing {
			Some(index) if list[index] == mount => return Ok(()),
			Some(index) => edit(list, |items| items[index] = mount),
			None => {
				let owned = counts.entry(Arc::clone(&mount.plugin_id)).or_default();
				if *owned >= MAX_MOUNTS_PER_PLUGIN {
					return Err(MountError::Limit);
				}
				*owned += 1;
				edit(list, |items| {
					items.push(mount);
					items.sort_by(|left, right| {
						(left.options.order, &left.plugin_id, &left.key).cmp(&(
							right.options.order,
							&right.plugin_id,
							&right.key,
						))
					});
				});
			}
		}
		drop(guard);
		self.bump_surfaces();
		Ok(())
	}

	pub fn update(
		&self,
		point: &'static str,
		plugin_id: &str,
		key: &str,
		tree: ViewTree,
	) -> Result<(), MountError> {
		let mut guard = self.inner.write();
		let list = guard.mounts.get_mut(point).ok_or(MountError::Missing)?;
		let index = list
			.iter()
			.position(|mount| &*mount.plugin_id == plugin_id && mount.key == key)
			.ok_or(MountError::Missing)?;
		if *list[index].tree == tree {
			return Ok(());
		}
		let tree = Arc::new(tree);
		edit(list, |items| items[index].tree = tree);
		drop(guard);
		self.bump_surfaces();
		Ok(())
	}

	pub fn unmount(&self, point: &'static str, plugin_id: &str, key: &str) {
		let mut guard = self.inner.write();
		let Some(list) = guard.mounts.get_mut(point) else {
			return;
		};
		let Some(index) = list
			.iter()
			.position(|mount| &*mount.plugin_id == plugin_id && mount.key == key)
		else {
			return;
		};
		edit(list, |items| {
			items.remove(index);
		});
		guard.release(plugin_id, 1);
		drop(guard);
		self.bump_surfaces();
	}

	pub fn provide(&self, point: &'static str, provider: Provider) {
		let mut guard = self.inner.write();
		let plugin_id = Arc::clone(&provider.plugin_id);
		let list = guard
			.providers
			.entry(point)
			.or_insert_with(|| Arc::clone(&NO_PROVIDERS));
		edit(list, |items| {
			match items
				.iter_mut()
				.find(|existing| existing.plugin_id == provider.plugin_id)
			{
				Some(existing) => *existing = provider,
				None => items.push(provider),
			}
			items.sort_by(|left, right| {
				(left.options.order, &left.plugin_id).cmp(&(right.options.order, &right.plugin_id))
			});
		});
		guard.restamp(&plugin_id, point);
		guard.views.retain(|(_, cached, _), _| *cached != point);
		drop(guard);
		self.bump_surfaces();
	}

	pub fn providers(&self, point: &str) -> Arc<[Provider]> {
		self.inner
			.read()
			.providers
			.get(point)
			.map_or_else(|| Arc::clone(&NO_PROVIDERS), Arc::clone)
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
			.flat_map(|providers| providers.iter())
			.map(|provider| {
				let cached = inner
					.views
					.get(&(Arc::clone(&provider.plugin_id), point, context))
					.cloned();
				(provider.clone(), cached)
			})
			.collect()
	}

	pub fn request(&self, point: &'static str, context: ViewContext) -> Vec<Requested> {
		let mut guard = self.inner.write();
		let inner = &mut *guard;
		let wanted: Vec<Requested> = inner
			.providers
			.get(point)
			.into_iter()
			.flat_map(|providers| providers.iter())
			.map(|provider| &provider.plugin_id)
			.filter(|plugin_id| !inner.views.contains_key(&(Arc::clone(plugin_id), point, context)))
			.map(|plugin_id| Requested {
				plugin_id: Arc::clone(plugin_id),
				stamp: inner.stamp_of(plugin_id, point),
			})
			.collect();
		if inner.views.len() + wanted.len() > MAX_CACHED_VIEWS {
			inner.views.clear();
		}
		for requested in &wanted {
			inner.views.insert(
				(Arc::clone(&requested.plugin_id), point, context),
				Contextual::Pending,
			);
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
		self.inner.write().views.insert(
			(Arc::clone(plugin_id), point, context),
			Contextual::Ready(tree.map(Arc::new)),
		);
		self.bump_surfaces();
	}

	pub fn store_requested_view(
		&self,
		requested: &Requested,
		point: &'static str,
		context: ViewContext,
		tree: Option<ViewTree>,
	) -> bool {
		let mut inner = self.inner.write();
		if inner.stamp_of(&requested.plugin_id, point) != requested.stamp {
			return false;
		}
		inner.views.insert(
			(Arc::clone(&requested.plugin_id), point, context),
			Contextual::Ready(tree.map(Arc::new)),
		);
		drop(inner);
		self.bump_surfaces();
		true
	}

	pub fn invalidate(&self, plugin_id: &Arc<str>, point: &'static str) {
		let mut inner = self.inner.write();
		inner.restamp(plugin_id, point);
		let before = inner.views.len();
		inner
			.views
			.retain(|(owner, cached, _), _| !(owner == plugin_id && *cached == point));
		let changed = inner.views.len() != before;
		drop(inner);
		if changed {
			self.bump_surfaces();
		}
	}

	pub fn set_menu(&self, menu: &'static str, entry: MenuEntry) {
		let mut inner = self.inner.write();
		let list = inner.menus.entry(menu).or_insert_with(|| Arc::clone(&NO_MENUS));
		edit(list, |items| {
			items.retain(|existing| existing.plugin_id != entry.plugin_id);
			if !entry.items.is_empty() {
				items.push(entry);
				items.sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));
			}
		});
		drop(inner);
		self.bump_surfaces();
	}

	pub fn menu_items(&self, menu: &str) -> Arc<[MenuEntry]> {
		self.inner
			.read()
			.menus
			.get(menu)
			.map_or_else(|| Arc::clone(&NO_MENUS), Arc::clone)
	}

	pub fn push_overlay(&self, overlay: Overlay) {
		let mut inner = self.inner.write();
		if inner.overlays.len() >= MAX_QUEUED_OVERLAYS {
			inner.overlays.pop_front();
		}
		inner.overlays.push_back(overlay);
		drop(inner);
		self.bump_overlays();
	}

	pub fn take_overlays(&self) -> Vec<Overlay> {
		if self.inner.read().overlays.is_empty() {
			return Vec::new();
		}
		self.inner.write().overlays.drain(..).collect()
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut inner = self.inner.write();
		for list in inner.mounts.values_mut() {
			if list.iter().any(|mount| &*mount.plugin_id == plugin_id) {
				edit(list, |items| items.retain(|mount| &*mount.plugin_id != plugin_id));
			}
		}
		for list in inner.providers.values_mut() {
			if list.iter().any(|provider| &*provider.plugin_id == plugin_id) {
				edit(list, |items| {
					items.retain(|provider| &*provider.plugin_id != plugin_id)
				});
			}
		}
		for list in inner.menus.values_mut() {
			if list.iter().any(|entry| &*entry.plugin_id == plugin_id) {
				edit(list, |items| items.retain(|entry| &*entry.plugin_id != plugin_id));
			}
		}
		inner.counts.remove(plugin_id);
		inner.stamps.retain(|(owner, _), _| &**owner != plugin_id);
		inner.views.retain(|(owner, _, _), _| &**owner != plugin_id);
		inner.overlays.retain(|overlay| match overlay {
			Overlay::Toast { plugin_id: owner, .. }
			| Overlay::Modal { plugin_id: owner, .. }
			| Overlay::Close { plugin_id: owner }
			| Overlay::Insert { plugin_id: owner, .. } => &**owner != plugin_id,
		});
		drop(inner);
		self.bump_surfaces();
		self.bump_overlays();
	}
}
