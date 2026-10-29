use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::api::model::{ChannelClick, ClickKind, ClickTarget, ClickWatch, MessageClick, Modifiers};

const MAX_WATCH_KINDS: usize = 3;
const MAX_WATCH_TARGETS: usize = 16;
const MAX_WATCHES: usize = 4;

#[derive(Default)]
struct Inner {
	messages: HashMap<Arc<str>, Vec<ClickWatch>>,
	channels: HashMap<Arc<str>, Vec<ClickWatch>>,
}

#[derive(Default)]
pub struct Watches {
	inner: RwLock<Inner>,
	revision: AtomicU64,
}

fn sanitized(watch: &ClickWatch) -> ClickWatch {
	let mut kinds: Vec<ClickKind> = Vec::with_capacity(MAX_WATCH_KINDS);
	for kind in &watch.kinds {
		if !kinds.contains(kind) {
			kinds.push(*kind);
		}
	}
	kinds.truncate(MAX_WATCH_KINDS);
	let mut targets = watch.targets.clone();
	targets.truncate(MAX_WATCH_TARGETS);
	ClickWatch {
		kinds,
		modifiers: watch.modifiers,
		targets,
	}
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

pub struct TooManyWatches;

fn store(
	map: &mut HashMap<Arc<str>, Vec<ClickWatch>>,
	plugin_id: &Arc<str>,
	watch: &ClickWatch,
) -> Result<bool, TooManyWatches> {
	let watch = sanitized(watch);
	let watches = map.entry(Arc::clone(plugin_id)).or_default();
	let existing = watches.iter().position(|kept| kept.modifiers == watch.modifiers);
	let outcome = match existing {
		Some(index) if watch.kinds.is_empty() => {
			watches.remove(index);
			Ok(true)
		}
		Some(index) if watches[index] == watch => Ok(false),
		Some(index) => {
			watches[index] = watch;
			Ok(true)
		}
		None if watch.kinds.is_empty() => Ok(false),
		None if watches.len() >= MAX_WATCHES => Err(TooManyWatches),
		None => {
			watches.push(watch);
			Ok(true)
		}
	};
	if watches.is_empty() {
		map.remove(&**plugin_id);
	}
	outcome
}

fn snapshot(map: &HashMap<Arc<str>, Vec<ClickWatch>>) -> Vec<(Arc<str>, ClickWatch)> {
	let mut found: Vec<_> = map
		.iter()
		.flat_map(|(plugin_id, watches)| watches.iter().map(|watch| (Arc::clone(plugin_id), watch.clone())))
		.collect();
	found.sort_by(|left, right| left.0.cmp(&right.0));
	found
}

fn matching(
	map: &HashMap<Arc<str>, Vec<ClickWatch>>,
	kind: ClickKind,
	modifiers: Modifiers,
	target: Option<&ClickTarget>,
) -> Vec<Arc<str>> {
	map.iter()
		.filter(|(_, watches)| {
			watches
				.iter()
				.any(|watch| matches(watch, kind, modifiers, target))
		})
		.map(|(plugin_id, _)| Arc::clone(plugin_id))
		.collect()
}

impl Watches {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn watch_messages(&self, plugin_id: &Arc<str>, watch: ClickWatch) -> Result<(), TooManyWatches> {
		let changed = store(&mut self.inner.write().messages, plugin_id, &watch)?;
		self.bump_if(changed);
		Ok(())
	}

	pub fn watch_channels(&self, plugin_id: &Arc<str>, watch: ClickWatch) -> Result<(), TooManyWatches> {
		let changed = store(&mut self.inner.write().channels, plugin_id, &watch)?;
		self.bump_if(changed);
		Ok(())
	}

	fn bump_if(&self, changed: bool) {
		if changed {
			self.revision.fetch_add(1, Ordering::AcqRel);
		}
	}

	pub fn watches_messages(&self) -> bool {
		!self.inner.read().messages.is_empty()
	}

	pub fn watches_channels(&self) -> bool {
		!self.inner.read().channels.is_empty()
	}

	pub fn message_watchers(&self) -> Vec<(Arc<str>, ClickWatch)> {
		snapshot(&self.inner.read().messages)
	}

	pub fn channel_watchers(&self) -> Vec<(Arc<str>, ClickWatch)> {
		snapshot(&self.inner.read().channels)
	}

	pub fn message_targets(&self, click: &MessageClick) -> Vec<Arc<str>> {
		matching(
			&self.inner.read().messages,
			click.kind,
			click.modifiers,
			Some(&click.target),
		)
	}

	pub fn channel_targets(&self, click: &ChannelClick) -> Vec<Arc<str>> {
		matching(&self.inner.read().channels, click.kind, click.modifiers, None)
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
