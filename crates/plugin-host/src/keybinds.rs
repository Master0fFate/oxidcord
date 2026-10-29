use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::api::model::Keybind;
use crate::convert::valid_key;

pub const MAX_KEYBINDS_PER_PLUGIN: usize = 32;

const MODIFIERS: [&str; 4] = ["ctrl", "alt", "shift", "meta"];
const KEYS: [&str; 57] = [
	"a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u",
	"v", "w", "x", "y", "z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "f1", "f2", "f3", "f4", "f5",
	"f6", "f7", "f8", "f9", "f10", "f11", "f12", "up", "down", "left", "right", "home", "end", "pageup",
	"pagedown", ",",
];
const CORE_CHORDS: [&str; 24] = [
	"ctrl+k", "meta+k", "ctrl+0", "meta+0", "alt+up", "alt+down", "alt+f4", "ctrl+a", "meta+a", "ctrl+c",
	"meta+c", "ctrl+v", "meta+v", "ctrl+x", "meta+x", "ctrl+z", "meta+z", "ctrl+y", "meta+y", "ctrl+w",
	"meta+w", "meta+q", "ctrl+,", "meta+,",
];

fn canonical_key(key: &str) -> Option<&'static str> {
	let key = match key {
		"pgup" => "pageup",
		"pgdn" => "pagedown",
		"comma" => ",",
		other => other,
	};
	KEYS.iter().copied().find(|known| *known == key)
}

pub fn normalize_chord(chord: &str) -> Option<String> {
	let lowered = chord.trim().to_ascii_lowercase();
	let mut held = [false; 4];
	let mut key = None;
	for part in lowered.split('+') {
		if let Some(index) = MODIFIERS.iter().position(|modifier| *modifier == part) {
			if std::mem::replace(&mut held[index], true) {
				return None;
			}
		} else if key.replace(canonical_key(part)?).is_some() {
			return None;
		}
	}
	let key = key?;
	let function = key.len() > 1 && key.starts_with('f') && key.as_bytes()[1].is_ascii_digit();
	if !function && !(held[0] || held[1] || held[3]) {
		return None;
	}
	let mut normalized = String::new();
	for (index, modifier) in MODIFIERS.iter().enumerate() {
		if held[index] {
			normalized.push_str(modifier);
			normalized.push('+');
		}
	}
	normalized.push_str(key);
	Some(normalized)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindError {
	Invalid,
	Reserved,
	Taken,
	Limit,
}

impl KeybindError {
	pub fn reason(self) -> &'static str {
		match self {
			Self::Invalid => "invalid keybind",
			Self::Reserved => "chord is reserved by the app",
			Self::Taken => "chord is bound by another plugin",
			Self::Limit => "too many keybinds",
		}
	}
}

#[derive(Default)]
struct Inner {
	by_chord: HashMap<String, (Arc<str>, String)>,
	by_plugin: HashMap<Arc<str>, Vec<(String, String)>>,
}

#[derive(Default)]
pub struct Keybinds {
	inner: RwLock<Inner>,
	revision: AtomicU64,
}

impl Keybinds {
	pub fn revision(&self) -> u64 {
		self.revision.load(Ordering::Acquire)
	}

	pub fn is_empty(&self) -> bool {
		self.inner.read().by_chord.is_empty()
	}

	pub fn lookup(&self, chord: &str) -> Option<(Arc<str>, String)> {
		let inner = self.inner.read();
		if inner.by_chord.is_empty() {
			return None;
		}
		inner.by_chord.get(&normalize_chord(chord)?).cloned()
	}

	pub fn register(&self, plugin_id: &Arc<str>, bind: &Keybind) -> Result<(), KeybindError> {
		if !valid_key(&bind.id) {
			return Err(KeybindError::Invalid);
		}
		let chord = normalize_chord(&bind.chord).ok_or(KeybindError::Invalid)?;
		if CORE_CHORDS.contains(&chord.as_str()) {
			return Err(KeybindError::Reserved);
		}
		let mut guard = self.inner.write();
		let inner = &mut *guard;
		if inner
			.by_chord
			.get(&chord)
			.is_some_and(|(owner, id)| !(owner == plugin_id && *id == bind.id))
		{
			return Err(KeybindError::Taken);
		}
		let owned = inner.by_plugin.entry(Arc::clone(plugin_id)).or_default();
		match owned.iter().position(|(id, _)| *id == bind.id) {
			Some(index) => {
				let previous = std::mem::replace(&mut owned[index].1, chord.clone());
				inner.by_chord.remove(&previous);
			}
			None if owned.len() >= MAX_KEYBINDS_PER_PLUGIN => return Err(KeybindError::Limit),
			None => owned.push((bind.id.clone(), chord.clone())),
		}
		inner
			.by_chord
			.insert(chord, (Arc::clone(plugin_id), bind.id.clone()));
		drop(guard);
		self.revision.fetch_add(1, Ordering::AcqRel);
		Ok(())
	}

	pub fn unregister(&self, plugin_id: &str, id: &str) -> bool {
		let mut guard = self.inner.write();
		let inner = &mut *guard;
		let Some(owned) = inner.by_plugin.get_mut(plugin_id) else {
			return false;
		};
		let Some(index) = owned.iter().position(|(owned_id, _)| owned_id == id) else {
			return false;
		};
		let (_, chord) = owned.remove(index);
		if owned.is_empty() {
			inner.by_plugin.remove(plugin_id);
		}
		inner.by_chord.remove(&chord);
		drop(guard);
		self.revision.fetch_add(1, Ordering::AcqRel);
		true
	}

	pub fn clear_plugin(&self, plugin_id: &str) {
		let mut guard = self.inner.write();
		let inner = &mut *guard;
		let Some(owned) = inner.by_plugin.remove(plugin_id) else {
			return;
		};
		for (_, chord) in owned {
			inner.by_chord.remove(&chord);
		}
		drop(guard);
		self.revision.fetch_add(1, Ordering::AcqRel);
	}
}
