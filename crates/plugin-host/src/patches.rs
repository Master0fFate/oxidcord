use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::api::model::{Annotation, Decoration, EmbedAction, EmbedCard, ViewTree, Visibility};

const MAX_PATCHED_MESSAGES: usize = 20_000;

#[derive(Debug, Clone, PartialEq)]
pub struct PluginPatch {
	pub plugin_id: Arc<str>,
	pub visibility: Visibility,
	pub annotations: Vec<Annotation>,
	pub decoration: Option<Decoration>,
	pub embeds: Vec<EmbedAction>,
	pub extra_embeds: Vec<EmbedCard>,
	pub hidden_attachments: Vec<u64>,
	pub accessory: Option<ViewTree>,
}

impl PluginPatch {
	pub fn is_empty(&self) -> bool {
		self.visibility == Visibility::Shown
			&& self.annotations.is_empty()
			&& self.decoration.is_none()
			&& self.embeds.iter().all(|action| *action == EmbedAction::Keep)
			&& self.extra_embeds.is_empty()
			&& self.hidden_attachments.is_empty()
			&& self.accessory.is_none()
	}
}

#[derive(Default)]
struct Inner {
	entries: HashMap<u64, Arc<[PluginPatch]>>,
	order: VecDeque<u64>,
}

#[derive(Default)]
pub struct Patches {
	inner: RwLock<Inner>,
	revision: AtomicU64,
}

impl Patches {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn get(&self, message_id: u64) -> Option<Arc<[PluginPatch]>> {
		self.inner.read().entries.get(&message_id).cloned()
	}

	pub fn set(&self, message_id: u64, patches: Vec<PluginPatch>) {
		let mut inner = self.inner.write();
		if patches.is_empty() {
			if inner.entries.remove(&message_id).is_none() {
				return;
			}
		} else {
			let patches: Arc<[PluginPatch]> = patches.into();
			if inner.entries.get(&message_id) == Some(&patches) {
				return;
			}
			if inner.entries.insert(message_id, patches).is_none() {
				inner.order.push_back(message_id);
				while inner.order.len() > MAX_PATCHED_MESSAGES {
					if let Some(oldest) = inner.order.pop_front() {
						inner.entries.remove(&oldest);
					}
				}
			}
		}
		drop(inner);
		self.revision.fetch_add(1, Ordering::AcqRel);
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut inner = self.inner.write();
		let mut changed = false;
		inner.entries.retain(|_, patches| {
			if patches.iter().all(|patch| &*patch.plugin_id != plugin_id) {
				return true;
			}
			changed = true;
			let kept: Vec<PluginPatch> = patches
				.iter()
				.filter(|patch| &*patch.plugin_id != plugin_id)
				.cloned()
				.collect();
			let keep = !kept.is_empty();
			*patches = kept.into();
			keep
		});
		let Inner { entries, order } = &mut *inner;
		order.retain(|id| entries.contains_key(id));
		drop(inner);
		if changed {
			self.revision.fetch_add(1, Ordering::AcqRel);
		}
	}
}
