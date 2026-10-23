use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Entity, Global, SharedString};
use plugin_host::{PluginStorage, SlotEntry};
use session::Command;
use store::Store;

use super::overlays::PluginUi;
use crate::data::Data;
use crate::shell::SettingsBackend;

#[derive(Default)]
struct Cache {
	revision: u64,
	slots: HashMap<String, Rc<Vec<SlotEntry>>>,
	names: HashMap<String, SharedString>,
	accessory: Rc<Vec<SlotEntry>>,
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

	pub fn entries(&self, slot: &str, cx: &App) -> Rc<Vec<SlotEntry>> {
		let slots = self.data.read(cx).session().plugin_slots();
		let revision = slots.revision();
		let mut cache = self.cache.borrow_mut();
		if cache.revision != revision {
			cache.slots.clear();
			cache.revision = revision;
		}
		if let Some(found) = cache.slots.get(slot) {
			return found.clone();
		}
		let entries: Rc<Vec<SlotEntry>> = Rc::new(
			slots
				.get(slot)
				.into_iter()
				.filter(|entry| !entry.tree.roots.is_empty())
				.collect(),
		);
		cache.slots.insert(slot.to_owned(), entries.clone());
		entries
	}

	pub fn accessory_changed(&self, cx: &App) -> bool {
		let current = self.entries("message-accessory", cx);
		let mut cache = self.cache.borrow_mut();
		let changed = *cache.accessory != *current;
		if changed {
			cache.accessory = current;
		}
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
