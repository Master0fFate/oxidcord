use std::thread;

use gpui::{App, BorrowAppContext as _, Global, WeakEntity};
use platform::hotkeys::{HotkeyEvent, Hotkeys};
use store::KeybindSettings;
use tokio::sync::mpsc::unbounded_channel;

use crate::data::Data;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bind {
	PushToTalk,
	ToggleMute,
	ToggleDeafen,
}

impl Bind {
	pub const ALL: [Bind; 3] = [Bind::PushToTalk, Bind::ToggleMute, Bind::ToggleDeafen];

	fn id(self) -> &'static str {
		match self {
			Self::PushToTalk => "push-to-talk",
			Self::ToggleMute => "toggle-mute",
			Self::ToggleDeafen => "toggle-deafen",
		}
	}

	fn from_id(id: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|bind| bind.id() == id)
	}

	pub fn label_key(self) -> &'static str {
		match self {
			Self::PushToTalk => "voice-keybind-push-to-talk",
			Self::ToggleMute => "voice-keybind-toggle-mute",
			Self::ToggleDeafen => "voice-keybind-toggle-deafen",
		}
	}

	pub fn get(self, keybinds: &KeybindSettings) -> Option<&str> {
		match self {
			Self::PushToTalk => keybinds.push_to_talk.as_deref(),
			Self::ToggleMute => keybinds.toggle_mute.as_deref(),
			Self::ToggleDeafen => keybinds.toggle_deafen.as_deref(),
		}
	}

	pub fn set(self, keybinds: &mut KeybindSettings, binding: Option<String>) {
		match self {
			Self::PushToTalk => keybinds.push_to_talk = binding,
			Self::ToggleMute => keybinds.toggle_mute = binding,
			Self::ToggleDeafen => keybinds.toggle_deafen = binding,
		}
	}
}

pub struct VoiceHotkeys {
	hotkeys: Option<Hotkeys>,
	target: Option<WeakEntity<Data>>,
}

impl Global for VoiceHotkeys {}

fn dispatch(event: HotkeyEvent, cx: &mut App) {
	let Some(bind) = Bind::from_id(&event.id) else {
		return;
	};
	let Some(data) = cx
		.try_global::<VoiceHotkeys>()
		.and_then(|hotkeys| hotkeys.target.as_ref())
		.and_then(WeakEntity::upgrade)
	else {
		return;
	};
	data.update(cx, |data, cx| match bind {
		Bind::PushToTalk => data.set_talking(event.pressed, cx),
		Bind::ToggleMute if event.pressed => data.toggle_mute(cx),
		Bind::ToggleDeafen if event.pressed => data.toggle_deafen(cx),
		Bind::ToggleMute | Bind::ToggleDeafen => {}
	});
}

impl VoiceHotkeys {
	pub fn install(keybinds: &KeybindSettings, cx: &mut App) {
		let hotkeys = match Hotkeys::new() {
			Ok((mut hotkeys, receiver)) => {
				for bind in Bind::ALL {
					if let Some(binding) = bind.get(keybinds)
						&& let Err(error) = hotkeys.register(binding, bind.id())
					{
						tracing::warn!(%error, binding, "could not register the hotkey");
					}
				}
				let (sender, mut events) = unbounded_channel::<HotkeyEvent>();
				thread::spawn(move || {
					while let Ok(event) = receiver.recv() {
						if sender.send(event).is_err() {
							break;
						}
					}
				});
				cx.spawn(async move |cx| {
					while let Some(event) = events.recv().await {
						cx.update(|cx| dispatch(event, cx));
					}
				})
				.detach();
				Some(hotkeys)
			}
			Err(error) => {
				tracing::warn!(%error, "global hotkeys are unavailable");
				None
			}
		};
		cx.set_global(Self {
			hotkeys,
			target: None,
		});
	}

	pub fn attach(data: WeakEntity<Data>, cx: &mut App) {
		if cx.has_global::<Self>() {
			cx.update_global::<Self, _>(|hotkeys, _| hotkeys.target = Some(data));
		}
	}

	pub fn bind(bind: Bind, binding: Option<&str>, cx: &mut App) -> Result<(), String> {
		if !cx.has_global::<Self>() {
			return Err("unavailable".to_owned());
		}
		cx.update_global::<Self, _>(|state, _| {
			let Some(hotkeys) = state.hotkeys.as_mut() else {
				return Err("unavailable".to_owned());
			};
			match binding {
				Some(binding) => hotkeys
					.register(binding, bind.id())
					.map_err(|error| error.to_string()),
				None => {
					let _ = hotkeys.unregister(bind.id());
					Ok(())
				}
			}
		})
	}
}
