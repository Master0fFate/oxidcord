use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::api::model::{ChannelClick, ClickKind, ClickTarget, ClickWatch, MessageClick, Modifiers};

const MAX_WATCH_KINDS: usize = 3;
const MAX_WATCH_TARGETS: usize = 16;

#[derive(Default)]
struct Inner {
	messages: HashMap<Arc<str>, ClickWatch>,
	channels: HashMap<Arc<str>, ClickWatch>,
}

#[derive(Default)]
pub struct Watches {
	inner: RwLock<Inner>,
	revision: AtomicU64,
}

fn sanitized(watch: ClickWatch) -> Option<ClickWatch> {
	let mut kinds: Vec<ClickKind> = Vec::with_capacity(MAX_WATCH_KINDS);
	for kind in watch.kinds {
		if !kinds.contains(&kind) {
			kinds.push(kind);
		}
	}
	kinds.truncate(MAX_WATCH_KINDS);
	let mut targets = watch.targets;
	targets.truncate(MAX_WATCH_TARGETS);
	(!kinds.is_empty()).then_some(ClickWatch {
		kinds,
		modifiers: watch.modifiers,
		targets,
	})
}

fn same_target(left: &ClickTarget, right: &ClickTarget) -> bool {
	std::mem::discriminant(left) == std::mem::discriminant(right)
}

fn matches(watch: &ClickWatch, kind: ClickKind, modifiers: Modifiers, target: Option<&ClickTarget>) -> bool {
	watch.kinds.contains(&kind)
		&& watch.modifiers == modifiers
		&& target.is_none_or(|target| {
			watch.targets.is_empty() || watch.targets.iter().any(|wanted| same_target(wanted, target))
		})
}

fn store(map: &mut HashMap<Arc<str>, ClickWatch>, plugin_id: &Arc<str>, watch: ClickWatch) {
	match sanitized(watch) {
		Some(watch) => map.insert(Arc::clone(plugin_id), watch),
		None => map.remove(&**plugin_id),
	};
}

fn snapshot(map: &HashMap<Arc<str>, ClickWatch>) -> Vec<(Arc<str>, ClickWatch)> {
	let mut found: Vec<_> = map
		.iter()
		.map(|(plugin_id, watch)| (Arc::clone(plugin_id), watch.clone()))
		.collect();
	found.sort_by(|left, right| left.0.cmp(&right.0));
	found
}

impl Watches {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn watch_messages(&self, plugin_id: &Arc<str>, watch: ClickWatch) {
		store(&mut self.inner.write().messages, plugin_id, watch);
		self.revision.fetch_add(1, Ordering::AcqRel);
	}

	pub fn watch_channels(&self, plugin_id: &Arc<str>, watch: ClickWatch) {
		store(&mut self.inner.write().channels, plugin_id, watch);
		self.revision.fetch_add(1, Ordering::AcqRel);
	}

	pub fn message_watchers(&self) -> Vec<(Arc<str>, ClickWatch)> {
		snapshot(&self.inner.read().messages)
	}

	pub fn channel_watchers(&self) -> Vec<(Arc<str>, ClickWatch)> {
		snapshot(&self.inner.read().channels)
	}

	pub fn message_targets(&self, click: &MessageClick) -> Vec<Arc<str>> {
		self.inner
			.read()
			.messages
			.iter()
			.filter(|(_, watch)| matches(watch, click.kind, click.modifiers, Some(&click.target)))
			.map(|(plugin_id, _)| Arc::clone(plugin_id))
			.collect()
	}

	pub fn channel_targets(&self, click: &ChannelClick) -> Vec<Arc<str>> {
		self.inner
			.read()
			.channels
			.iter()
			.filter(|(_, watch)| matches(watch, click.kind, click.modifiers, None))
			.map(|(plugin_id, _)| Arc::clone(plugin_id))
			.collect()
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut inner = self.inner.write();
		let removed = inner.messages.remove(plugin_id).is_some() | inner.channels.remove(plugin_id).is_some();
		drop(inner);
		if removed {
			self.revision.fetch_add(1, Ordering::AcqRel);
		}
	}
}
