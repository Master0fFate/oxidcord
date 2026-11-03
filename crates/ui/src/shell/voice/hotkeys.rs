use std::rc::Rc;
use std::thread;

use gpui::{App, BorrowAppContext as _, Entity, Global, WeakEntity};
use platform::hotkeys::{HotkeyEvent, Hotkeys};
use store::KeybindSettings;
use tokio::sync::mpsc::unbounded_channel;

use super::quality::Preset;
use super::{actions, picker};
use crate::data::Data;
use crate::shell::SettingsBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bind {
	PushToTalk,
	ToggleMute,
	ToggleDeafen,
	ToggleCamera,
	ToggleScreenShare,
	PushToMute,
	PriorityPushToTalk,
}

impl Bind {
	pub const ALL: [Bind; 7] = [
		Bind::PushToTalk,
		Bind::PriorityPushToTalk,
		Bind::PushToMute,
		Bind::ToggleMute,
		Bind::ToggleDeafen,
		Bind::ToggleCamera,
		Bind::ToggleScreenShare,
	];

	fn id(self) -> &'static str {
		match self {
			Self::PushToTalk => "push-to-talk",
			Self::ToggleMute => "toggle-mute",
			Self::ToggleDeafen => "toggle-deafen",
			Self::ToggleCamera => "toggle-camera",
			Self::ToggleScreenShare => "toggle-screen-share",
			Self::PushToMute => "push-to-mute",
			Self::PriorityPushToTalk => "priority-push-to-talk",
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
			Self::ToggleCamera => "voice-keybind-toggle-camera",
			Self::ToggleScreenShare => "voice-keybind-toggle-screen-share",
			Self::PushToMute => "voice-keybind-push-to-mute",
			Self::PriorityPushToTalk => "voice-keybind-priority-push-to-talk",
		}
	}

	pub fn get(self, keybinds: &KeybindSettings) -> Option<&str> {
		match self {
			Self::PushToTalk => keybinds.push_to_talk.as_deref(),
			Self::ToggleMute => keybinds.toggle_mute.as_deref(),
			Self::ToggleDeafen => keybinds.toggle_deafen.as_deref(),
			Self::ToggleCamera => keybinds.toggle_camera.as_deref(),
			Self::ToggleScreenShare => keybinds.toggle_screen_share.as_deref(),
			Self::PushToMute => keybinds.push_to_mute.as_deref(),
			Self::PriorityPushToTalk => keybinds.priority_push_to_talk.as_deref(),
		}
	}

	pub fn set(self, keybinds: &mut KeybindSettings, binding: Option<String>) {
		match self {
			Self::PushToTalk => keybinds.push_to_talk = binding,
			Self::ToggleMute => keybinds.toggle_mute = binding,
			Self::ToggleDeafen => keybinds.toggle_deafen = binding,
			Self::ToggleCamera => keybinds.toggle_camera = binding,
			Self::ToggleScreenShare => keybinds.toggle_screen_share = binding,
			Self::PushToMute => keybinds.push_to_mute = binding,
			Self::PriorityPushToTalk => keybinds.priority_push_to_talk = binding,
		}
	}
}

struct Target {
	data: WeakEntity<Data>,
	backend: Rc<SettingsBackend>,
}

pub struct VoiceHotkeys {
	hotkeys: Option<Hotkeys>,
	target: Option<Target>,
}

impl Global for VoiceHotkeys {}

fn dispatch(event: HotkeyEvent, cx: &mut App) {
	let Some(bind) = Bind::from_id(&event.id) else {
		return;
	};
	let Some((data, backend)) = cx
		.try_global::<VoiceHotkeys>()
		.and_then(|hotkeys| hotkeys.target.as_ref())
		.and_then(|target| Some((target.data.upgrade()?, target.backend.clone())))
	else {
		return;
	};
	match bind {
		Bind::PushToTalk => data.update(cx, |data, cx| data.set_talking(event.pressed, cx)),
		Bind::PriorityPushToTalk => data.read(cx).set_priority_talking(event.pressed),
		Bind::PushToMute => data.read(cx).set_push_muted(event.pressed),
		Bind::ToggleMute if event.pressed => data.update(cx, |data, cx| data.toggle_mute(cx)),
		Bind::ToggleDeafen if event.pressed => data.update(cx, |data, cx| data.toggle_deafen(cx)),
		Bind::ToggleCamera if event.pressed && in_voice(&data, cx) => {
			actions::toggle_camera(&data, &backend, cx);
		}
		Bind::ToggleScreenShare if event.pressed && in_voice(&data, cx) => {
			toggle_screen_share(&data, &backend, cx);
		}
		Bind::ToggleMute | Bind::ToggleDeafen | Bind::ToggleCamera | Bind::ToggleScreenShare => {}
	}
}

fn in_voice(data: &Entity<Data>, cx: &App) -> bool {
	data.read(cx).state().voice().is_some()
}

fn toggle_screen_share(data: &Entity<Data>, backend: &SettingsBackend, cx: &mut App) {
	if data
		.read(cx)
		.state()
		.voice()
		.is_some_and(|voice| voice.screen_share.is_some())
	{
		actions::stop_screen_share(data, cx);
		return;
	}
	let preset = Preset::from_index((backend.read)().voice.video_preset);
	if let Some(window) = cx.windows().into_iter().next() {
		let _ = window.update(cx, |_, window, cx| picker::open(data, preset, window, cx));
	}
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

	pub fn attach(data: WeakEntity<Data>, backend: Rc<SettingsBackend>, cx: &mut App) {
		if cx.has_global::<Self>() {
			cx.update_global::<Self, _>(|hotkeys, _| hotkeys.target = Some(Target { data, backend }));
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
