use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::api::{UiNode, UiTree};

#[derive(Debug, Clone, PartialEq)]
pub struct SlotEntry {
	pub plugin_id: Arc<str>,
	pub tree: UiTree,
}

#[derive(Default)]
pub struct Slots {
	entries: RwLock<HashMap<String, Vec<SlotEntry>>>,
	revision: AtomicU64,
}

impl Slots {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn get(&self, slot: &str) -> Vec<SlotEntry> {
		self.entries.read().get(slot).cloned().unwrap_or_default()
	}

	pub fn names(&self) -> Vec<String> {
		self.entries.read().keys().cloned().collect()
	}

	pub fn set(&self, plugin_id: &Arc<str>, slot: &str, tree: UiTree) {
		let mut entries = self.entries.write();
		let list = entries.entry(slot.to_owned()).or_default();
		match list.iter_mut().find(|entry| entry.plugin_id == *plugin_id) {
			Some(entry) => entry.tree = tree,
			None => list.push(SlotEntry {
				plugin_id: Arc::clone(plugin_id),
				tree,
			}),
		}
		self.bump();
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut entries = self.entries.write();
		let mut changed = false;
		entries.retain(|_, list| {
			let before = list.len();
			list.retain(|entry| &*entry.plugin_id != plugin_id);
			changed |= list.len() != before;
			!list.is_empty()
		});
		drop(entries);
		if changed {
			self.bump();
		}
	}

	pub fn replace_owner_of(&self, plugin_id: &str, action_id: &str, tree: UiTree) -> bool {
		let mut entries = self.entries.write();
		let owner = entries
			.values_mut()
			.flatten()
			.find(|entry| &*entry.plugin_id == plugin_id && owns_id(&entry.tree, action_id));
		let Some(owner) = owner else {
			return false;
		};
		owner.tree = tree;
		drop(entries);
		self.bump();
		true
	}

	fn bump(&self) {
		self.revision.fetch_add(1, Ordering::AcqRel);
	}
}

fn owns_id(tree: &UiTree, action_id: &str) -> bool {
	tree.nodes.iter().any(|node| match node {
		UiNode::Column(layout) | UiNode::Row(layout) => layout.id.as_deref() == Some(action_id),
		UiNode::Button(button) => button.id == action_id,
		UiNode::Input(input) => input.id == action_id,
		UiNode::Toggle(toggle) => toggle.id == action_id,
		_ => false,
	})
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
	pub plugin_id: Arc<str>,
	pub description: String,
}

#[derive(Default)]
pub struct Commands {
	entries: RwLock<HashMap<String, CommandEntry>>,
}

impl Commands {
	pub fn register(&self, plugin_id: &Arc<str>, name: &str, description: &str) -> bool {
		let mut entries = self.entries.write();
		if let Some(existing) = entries.get(name)
			&& existing.plugin_id != *plugin_id
		{
			return false;
		}
		entries.insert(
			name.to_owned(),
			CommandEntry {
				plugin_id: Arc::clone(plugin_id),
				description: description.to_owned(),
			},
		);
		true
	}

	pub fn owner(&self, name: &str) -> Option<Arc<str>> {
		self.entries
			.read()
			.get(name)
			.map(|entry| Arc::clone(&entry.plugin_id))
	}

	pub fn list(&self) -> Vec<(String, CommandEntry)> {
		let mut commands: Vec<_> = self
			.entries
			.read()
			.iter()
			.map(|(name, entry)| (name.clone(), entry.clone()))
			.collect();
		commands.sort_by(|left, right| left.0.cmp(&right.0));
		commands
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		self.entries
			.write()
			.retain(|_, entry| &*entry.plugin_id != plugin_id);
	}
}
