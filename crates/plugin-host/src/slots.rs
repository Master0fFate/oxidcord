use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use parking_lot::RwLock;

use crate::api::model::ViewTree;
use crate::convert::has_key;

static NO_ENTRIES: LazyLock<Arc<[SlotEntry]>> = LazyLock::new(|| Arc::from([]));
static NO_COMMANDS: LazyLock<Arc<[(String, CommandEntry)]>> = LazyLock::new(|| Arc::from([]));

#[derive(Debug, Clone, PartialEq)]
pub struct SlotEntry {
	pub plugin_id: Arc<str>,
	pub tree: Arc<ViewTree>,
}

#[derive(Default)]
pub struct Slots {
	entries: RwLock<HashMap<String, Arc<[SlotEntry]>>>,
	revision: AtomicU64,
}

impl Slots {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn get(&self, slot: &str) -> Arc<[SlotEntry]> {
		self.entries
			.read()
			.get(slot)
			.map_or_else(|| Arc::clone(&NO_ENTRIES), Arc::clone)
	}

	pub fn names(&self) -> Vec<String> {
		self.entries.read().keys().cloned().collect()
	}

	pub fn set(&self, plugin_id: &Arc<str>, slot: &str, tree: ViewTree) {
		let mut entries = self.entries.write();
		let list = entries
			.entry(slot.to_owned())
			.or_insert_with(|| Arc::clone(&NO_ENTRIES));
		let mut items = list.to_vec();
		match items.iter_mut().find(|entry| entry.plugin_id == *plugin_id) {
			Some(entry) if *entry.tree == tree => return,
			Some(entry) => entry.tree = Arc::new(tree),
			None => items.push(SlotEntry {
				plugin_id: Arc::clone(plugin_id),
				tree: Arc::new(tree),
			}),
		}
		*list = items.into();
		drop(entries);
		self.bump();
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut entries = self.entries.write();
		let mut changed = false;
		entries.retain(|_, list| {
			if list.iter().all(|entry| &*entry.plugin_id != plugin_id) {
				return true;
			}
			changed = true;
			let kept: Vec<SlotEntry> = list
				.iter()
				.filter(|entry| &*entry.plugin_id != plugin_id)
				.cloned()
				.collect();
			*list = kept.into();
			!list.is_empty()
		});
		drop(entries);
		if changed {
			self.bump();
		}
	}

	pub fn replace_owner_of(&self, plugin_id: &str, action_id: &str, tree: ViewTree) -> bool {
		let mut entries = self.entries.write();
		let found = entries.values_mut().find_map(|list| {
			list.iter()
				.position(|entry| &*entry.plugin_id == plugin_id && has_key(&entry.tree, action_id))
				.map(|index| (list, index))
		});
		let Some((list, index)) = found else {
			return false;
		};
		if *list[index].tree == tree {
			return true;
		}
		let mut items = list.to_vec();
		items[index].tree = Arc::new(tree);
		*list = items.into();
		drop(entries);
		self.bump();
		true
	}

	fn bump(&self) {
		self.revision.fetch_add(1, Ordering::AcqRel);
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
	pub plugin_id: Arc<str>,
	pub description: String,
}

#[derive(Default)]
struct Table {
	entries: HashMap<String, CommandEntry>,
	sorted: Option<Arc<[(String, CommandEntry)]>>,
}

impl Table {
	fn rebuild(&mut self) {
		let mut commands: Vec<_> = self
			.entries
			.iter()
			.map(|(name, entry)| (name.clone(), entry.clone()))
			.collect();
		commands.sort_by(|left, right| left.0.cmp(&right.0));
		self.sorted = Some(commands.into());
	}
}

#[derive(Default)]
pub struct Commands {
	table: RwLock<Table>,
	revision: AtomicU64,
}

impl Commands {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn register(&self, plugin_id: &Arc<str>, name: &str, description: &str) -> bool {
		let mut table = self.table.write();
		match table.entries.get(name) {
			Some(existing) if existing.plugin_id != *plugin_id => return false,
			Some(existing) if existing.description == description => return true,
			_ => {}
		}
		table.entries.insert(
			name.to_owned(),
			CommandEntry {
				plugin_id: Arc::clone(plugin_id),
				description: description.to_owned(),
			},
		);
		table.sorted = None;
		drop(table);
		self.revision.fetch_add(1, Ordering::AcqRel);
		true
	}

	pub fn owner(&self, name: &str) -> Option<Arc<str>> {
		self.table
			.read()
			.entries
			.get(name)
			.map(|entry| Arc::clone(&entry.plugin_id))
	}

	pub fn list(&self) -> Arc<[(String, CommandEntry)]> {
		if let Some(sorted) = &self.table.read().sorted {
			return Arc::clone(sorted);
		}
		let mut table = self.table.write();
		if table.entries.is_empty() {
			return Arc::clone(&NO_COMMANDS);
		}
		if table.sorted.is_none() {
			table.rebuild();
		}
		table
			.sorted
			.as_ref()
			.map_or_else(|| Arc::clone(&NO_COMMANDS), Arc::clone)
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut table = self.table.write();
		let before = table.entries.len();
		table.entries.retain(|_, entry| &*entry.plugin_id != plugin_id);
		let changed = table.entries.len() != before;
		if changed {
			table.sorted = None;
		}
		drop(table);
		if changed {
			self.revision.fetch_add(1, Ordering::AcqRel);
		}
	}
}
