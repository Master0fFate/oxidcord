use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use parking_lot::Mutex;

use crate::{PlatformError, Result};

static CREATED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyEvent {
	pub id: String,
	pub pressed: bool,
}

pub struct Hotkeys {
	manager: GlobalHotKeyManager,
	registered: HashMap<String, HotKey>,
	ids: Arc<Mutex<HashMap<u32, String>>>,
}

impl Hotkeys {
	pub fn new() -> Result<(Hotkeys, Receiver<HotkeyEvent>)> {
		if CREATED.swap(true, Ordering::SeqCst) {
			return Err(PlatformError::AlreadyCreated("hotkeys"));
		}
		let manager = GlobalHotKeyManager::new()?;
		let ids: Arc<Mutex<HashMap<u32, String>>> = Arc::default();
		let (sender, receiver) = mpsc::channel();

		let handler_ids = Arc::clone(&ids);
		GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
			if let Some(id) = handler_ids.lock().get(&event.id()) {
				let _ = sender.send(HotkeyEvent {
					id: id.clone(),
					pressed: event.state() == HotKeyState::Pressed,
				});
			}
		}));

		Ok((
			Hotkeys {
				manager,
				registered: HashMap::new(),
				ids,
			},
			receiver,
		))
	}

	pub fn register(&mut self, binding: &str, id: &str) -> Result<()> {
		let hotkey: HotKey = binding.parse()?;
		self.unregister(id).ok();
		self.manager.register(hotkey)?;
		self.ids.lock().insert(hotkey.id(), id.to_owned());
		self.registered.insert(id.to_owned(), hotkey);
		Ok(())
	}

	pub fn unregister(&mut self, id: &str) -> Result<()> {
		let hotkey = self
			.registered
			.remove(id)
			.ok_or_else(|| PlatformError::UnknownHotkey(id.to_owned()))?;
		self.ids.lock().remove(&hotkey.id());
		self.manager.unregister(hotkey)?;
		Ok(())
	}
}
