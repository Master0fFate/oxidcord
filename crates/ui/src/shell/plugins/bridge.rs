use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Entity, Global, SharedString};
use plugin_host::api::model::{MenuItem, ViewTree};
use plugin_host::mounts::{Contextual, MenuEntry};
use plugin_host::{PluginPatch, PluginStorage, ViewContext};
use session::Command;
use store::Store;

use super::overlays::PluginUi;
use crate::data::Data;
use crate::shell::SettingsBackend;

const MAX_CACHED_PATCHES: usize = 512;

#[derive(Clone)]
pub struct Surface {
	pub plugin_id: Arc<str>,
	pub key: SharedString,
	pub tree: Arc<ViewTree>,
	pub title: SharedString,
	pub icon: Option<SharedString>,
	pub legacy: bool,
}

pub struct PatchSet {
	pub patches: Arc<[PluginPatch]>,
	pub accessories: Vec<Option<Arc<ViewTree>>>,
}

pub type Patched = Rc<PatchSet>;

pub struct MenuGroup {
	pub plugin_id: Arc<str>,
	pub items: Arc<[MenuItem]>,
}

#[derive(Default)]
struct Cache {
	slots_revision: u64,
	surfaces_revision: u64,
	slots: HashMap<String, Rc<Vec<Surface>>>,
	mounts: HashMap<String, Rc<Vec<Surface>>>,
	contextual: HashMap<(&'static str, ViewContext), Rc<Vec<Surface>>>,
	requested: HashSet<(&'static str, ViewContext)>,
	menus: HashMap<&'static str, Rc<Vec<MenuGroup>>>,
	patches_revision: u64,
	patches: HashMap<u64, Patched>,
	names: HashMap<String, SharedString>,
	accessory: Rc<Vec<Surface>>,
	forms: HashMap<SharedString, HashMap<String, String>>,
	generations: HashMap<SharedString, u64>,
}

pub struct PluginStore(pub Arc<Store>);

impl Global for PluginStore {}

pub struct PluginBridge {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	ui: Entity<PluginUi>,
	cache: RefCell<Cache>,
	dirty: RefCell<HashMap<(Arc<str>, SharedString), SharedString>>,
}

impl Global for PluginBridge {}

fn surface_of(
	plugin_id: &Arc<str>,
	key: String,
	tree: Arc<ViewTree>,
	title: String,
	icon: Option<String>,
	legacy: bool,
) -> Surface {
	Surface {
		plugin_id: Arc::clone(plugin_id),
		key: key.into(),
		tree,
		title: title.into(),
		icon: icon.map(Into::into),
		legacy,
	}
}

impl PluginBridge {
	pub fn new(data: Entity<Data>, backend: Rc<SettingsBackend>, ui: Entity<PluginUi>) -> Self {
		Self {
			data,
			backend,
			ui,
			cache: RefCell::new(Cache::default()),
			dirty: RefCell::new(HashMap::new()),
		}
	}

	pub fn data(&self) -> &Entity<Data> {
		&self.data
	}

	pub fn backend(&self) -> &Rc<SettingsBackend> {
		&self.backend
	}

	pub fn ui(&self) -> &Entity<PluginUi> {
		&self.ui
	}

	pub fn send(&self, command: Command, cx: &App) {
		self.data.read(cx).send(command);
	}

	fn fresh(&self, cx: &App) -> std::cell::RefMut<'_, Cache> {
		let session = self.data.read(cx).session();
		let slots_revision = session.plugin_slots().revision();
		let surfaces_revision = session.plugin_mounts().surfaces_revision();
		let mut cache = self.cache.borrow_mut();
		if cache.slots_revision != slots_revision {
			cache.slots_revision = slots_revision;
			cache.slots.clear();
		}
		if cache.surfaces_revision != surfaces_revision {
			cache.surfaces_revision = surfaces_revision;
			cache.mounts.clear();
			cache.contextual.clear();
			cache.requested.clear();
			cache.menus.clear();
		}
		cache
	}

	pub fn slot_entries(&self, slot: &str, cx: &App) -> Rc<Vec<Surface>> {
		let mut cache = self.fresh(cx);
		if let Some(found) = cache.slots.get(slot) {
			return found.clone();
		}
		let entries: Rc<Vec<Surface>> = Rc::new(
			self.data
				.read(cx)
				.session()
				.plugin_slots()
				.get(slot)
				.iter()
				.filter(|entry| !entry.tree.roots.is_empty())
				.map(|entry| {
					surface_of(
						&entry.plugin_id,
						String::new(),
						Arc::clone(&entry.tree),
						String::new(),
						None,
						true,
					)
				})
				.collect(),
		);
		cache.slots.insert(slot.to_owned(), entries.clone());
		entries
	}

	pub fn mounts(&self, point: &str, cx: &App) -> Rc<Vec<Surface>> {
		let mut cache = self.fresh(cx);
		if let Some(found) = cache.mounts.get(point) {
			return found.clone();
		}
		let entries: Rc<Vec<Surface>> = Rc::new(
			self.data
				.read(cx)
				.session()
				.plugin_mounts()
				.mounted(point)
				.iter()
				.map(|mount| {
					surface_of(
						&mount.plugin_id,
						mount.key.clone(),
						Arc::clone(&mount.tree),
						mount.options.title.clone(),
						mount.options.icon.clone(),
						false,
					)
				})
				.collect(),
		);
		cache.mounts.insert(point.to_owned(), entries.clone());
		entries
	}

	pub fn surfaces(&self, point: &str, cx: &App) -> Rc<Vec<Surface>> {
		let mounted = self.mounts(point, cx);
		let slots = self.slot_entries(point, cx);
		if mounted.is_empty() {
			return slots;
		}
		if slots.is_empty() {
			return mounted;
		}
		Rc::new(slots.iter().chain(mounted.iter()).cloned().collect())
	}

	pub fn contextual(&self, point: &'static str, context: ViewContext, cx: &App) -> Rc<Vec<Surface>> {
		let mut cache = self.fresh(cx);
		if let Some(found) = cache.contextual.get(&(point, context)) {
			return found.clone();
		}
		let mut missing = false;
		let entries: Rc<Vec<Surface>> = Rc::new(
			self.data
				.read(cx)
				.session()
				.plugin_mounts()
				.contextual(point, context)
				.into_iter()
				.filter_map(|(provider, cached)| match cached {
					Some(Contextual::Ready(Some(tree))) => Some(surface_of(
						&provider.plugin_id,
						String::new(),
						tree,
						provider.options.title,
						provider.options.icon,
						false,
					)),
					Some(Contextual::Ready(None) | Contextual::Pending) => None,
					None => {
						missing = true;
						None
					}
				})
				.collect(),
		);
		if missing && cache.requested.insert((point, context)) {
			self.data.read(cx).send(Command::PluginRenderView {
				point: point.to_owned(),
				context,
			});
		}
		cache.contextual.insert((point, context), entries.clone());
		entries
	}

	pub fn menu_groups(&self, menu: &'static str, cx: &App) -> Rc<Vec<MenuGroup>> {
		let mut cache = self.fresh(cx);
		if let Some(found) = cache.menus.get(menu) {
			return found.clone();
		}
		let groups: Rc<Vec<MenuGroup>> = Rc::new(
			self.data
				.read(cx)
				.session()
				.plugin_mounts()
				.menu_items(menu)
				.iter()
				.map(|MenuEntry { plugin_id, items, .. }| MenuGroup {
					plugin_id: Arc::clone(plugin_id),
					items: Arc::clone(items),
				})
				.collect(),
		);
		cache.menus.insert(menu, groups.clone());
		groups
	}

	pub fn patches(&self, message_id: u64, cx: &App) -> Option<Patched> {
		let store = self.data.read(cx).session().plugin_patches();
		let revision = store.revision();
		let mut cache = self.cache.borrow_mut();
		if cache.patches_revision != revision {
			cache.patches_revision = revision;
			if cache.patches.len() > MAX_CACHED_PATCHES {
				cache.patches.clear();
			}
		}
		let Some(current) = store.get(message_id) else {
			cache.patches.remove(&message_id);
			return None;
		};
		if let Some(patched) = cache.patches.get(&message_id)
			&& Arc::ptr_eq(&patched.patches, &current)
		{
			return Some(patched.clone());
		}
		let patched: Patched = Rc::new(PatchSet {
			accessories: current.iter().map(|patch| patch.accessory.clone()).collect(),
			patches: current,
		});
		cache.patches.insert(message_id, patched.clone());
		Some(patched)
	}

	pub fn accessory_changed(&self, cx: &App) -> bool {
		let current = self.slot_entries("message-accessory", cx);
		let mut cache = self.cache.borrow_mut();
		if Rc::ptr_eq(&cache.accessory, &current) {
			return false;
		}
		let changed = cache.accessory.len() != current.len()
			|| cache.accessory.iter().zip(current.iter()).any(|(left, right)| {
				left.plugin_id != right.plugin_id
					|| !(Arc::ptr_eq(&left.tree, &right.tree) || left.tree == right.tree)
			});
		cache.accessory = current;
		changed
	}

	pub fn forget_names(&self) {
		self.cache.borrow_mut().names.clear();
	}

	pub fn name_of(&self, plugin_id: &str, cx: &App) -> SharedString {
		if let Some(name) = self.cache.borrow().names.get(plugin_id) {
			return name.clone();
		}
		let list = self.data.read(cx).session().plugin_list();
		let mut cache = self.cache.borrow_mut();
		for info in list {
			cache.names.insert(info.id, info.name.into());
		}
		cache
			.names
			.get(plugin_id)
			.cloned()
			.unwrap_or_else(|| plugin_id.to_owned().into())
	}

	pub fn record_field(&self, form: &SharedString, key: &str, value: String) {
		self.cache
			.borrow_mut()
			.forms
			.entry(form.clone())
			.or_default()
			.insert(key.to_owned(), value);
	}

	pub fn take_form(&self, form: &SharedString) -> HashMap<String, String> {
		let mut cache = self.cache.borrow_mut();
		*cache.generations.entry(form.clone()).or_default() += 1;
		cache.forms.remove(form).unwrap_or_default()
	}

	pub fn form_generation(&self, form: &SharedString) -> u64 {
		self.cache.borrow().generations.get(form).copied().unwrap_or(0)
	}
}

pub fn act(plugin: &str, id: &str, payload: &str, cx: &mut App) {
	let (data, ui) = {
		let bridge = cx.global::<PluginBridge>();
		(bridge.data.clone(), bridge.ui.clone())
	};
	data.read(cx).send(Command::PluginUiAction {
		plugin_id: plugin.to_owned(),
		id: id.to_owned(),
		payload: payload.to_owned(),
	});
	let plugin = plugin.to_owned();
	ui.update(cx, |ui, cx| ui.action_sent(&plugin, cx));
}

pub fn stored_setting(plugin_id: &str, key: &str, cx: &App) -> Option<String> {
	let store = cx.try_global::<PluginStore>()?.0.clone();
	let bytes = PluginStorage::new(store, plugin_id)
		.get(&format!("settings.{key}"))
		.ok()??;
	String::from_utf8(bytes).ok().filter(|value| !value.is_empty())
}

pub fn remember_anchor(at: gpui::Point<gpui::Pixels>, cx: &mut App) {
	let ui = cx.global::<PluginBridge>().ui.clone();
	ui.update(cx, |ui, _| ui.remember_anchor(at));
}

pub fn note_input(plugin: &Arc<str>, id: &SharedString, text: SharedString, cx: &App) {
	cx.global::<PluginBridge>()
		.dirty
		.borrow_mut()
		.insert((Arc::clone(plugin), id.clone()), text);
}

pub fn forget_input(plugin: &Arc<str>, id: &SharedString, cx: &App) {
	cx.global::<PluginBridge>()
		.dirty
		.borrow_mut()
		.remove(&(Arc::clone(plugin), id.clone()));
}

pub fn flush_inputs(plugin: &Arc<str>, cx: &mut App) {
	let pending: Vec<(SharedString, SharedString)> = {
		let mut dirty = cx.global::<PluginBridge>().dirty.borrow_mut();
		let keys: Vec<_> = dirty
			.keys()
			.filter(|(owner, _)| owner == plugin)
			.cloned()
			.collect();
		keys.into_iter()
			.filter_map(|key| dirty.remove(&key).map(|text| (key.1, text)))
			.collect()
	};
	for (id, text) in pending {
		act(plugin, &id, &text, cx);
	}
}
