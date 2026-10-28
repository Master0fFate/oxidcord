use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{App, Context, EventEmitter};
use parking_lot::RwLockReadGuard;
use protocol::Snowflake;
use session::{Command, InputMode, SearchResults, Session};
use state::{Changes, State};
use store::StreamerModeSettings;

use crate::shell::PluginStore;

pub fn streamer_mode(cx: &App) -> StreamerModeSettings {
	let mut settings = cx
		.try_global::<PluginStore>()
		.map(|store| store.0.settings.read().streamer_mode.clone())
		.unwrap_or_default();
	match crate::shell::client_options(cx).streamer_mode {
		Some(true) => {
			settings.enabled = true;
			settings.hide_personal = true;
			settings.hide_invites = true;
			settings.disable_sounds = true;
		}
		Some(false) => settings.enabled = false,
		None => {}
	}
	settings
}

pub enum DataEvent {
	Changed(Box<Changes>),
	MediaReady,
}

#[derive(Default)]
struct LocalVoice {
	self_mute: bool,
	self_deaf: bool,
	push_to_talk: bool,
	talking: bool,
	volumes: HashMap<Snowflake, f32>,
	silenced: HashSet<Snowflake>,
}

pub struct Data {
	session: Session,
	local_voice: LocalVoice,
}

impl EventEmitter<DataEvent> for Data {}

impl Data {
	pub fn new(session: Session) -> Self {
		Self {
			session,
			local_voice: LocalVoice::default(),
		}
	}

	pub fn state(&self) -> RwLockReadGuard<'_, State> {
		self.session.state().read()
	}

	pub fn me(&self) -> Option<Snowflake> {
		self.state().current_user_id()
	}

	pub fn send(&self, command: Command) {
		self.session.send(command);
	}

	pub fn session(&self) -> &Session {
		&self.session
	}

	pub fn self_mute(&self) -> bool {
		self.state()
			.voice()
			.map_or(self.local_voice.self_mute, |voice| voice.self_mute)
	}

	pub fn self_deaf(&self) -> bool {
		self.state()
			.voice()
			.map_or(self.local_voice.self_deaf, |voice| voice.self_deaf)
	}

	pub fn push_to_talk(&self) -> bool {
		self.local_voice.push_to_talk
	}

	pub fn talking(&self) -> bool {
		self.local_voice.talking
	}

	pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
		let muted = !self.self_mute();
		self.local_voice.self_mute = muted;
		self.session.send(Command::SetSelfMute(muted));
		cx.notify();
	}

	pub fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
		let deafened = !self.self_deaf();
		self.local_voice.self_deaf = deafened;
		self.session.send(Command::SetSelfDeaf(deafened));
		cx.notify();
	}

	pub fn restore_input_mode(&mut self, push_to_talk: bool) {
		self.local_voice.push_to_talk = push_to_talk;
	}

	pub fn set_input_mode(&mut self, push_to_talk: bool, cx: &mut Context<Self>) {
		self.local_voice.push_to_talk = push_to_talk;
		self.session.send(Command::SetInputMode(if push_to_talk {
			InputMode::PushToTalk
		} else {
			InputMode::VoiceActivity
		}));
		cx.notify();
	}

	pub fn set_talking(&mut self, talking: bool, cx: &mut Context<Self>) {
		self.local_voice.talking = talking;
		self.session.send(Command::SetPushToTalk(talking));
		cx.notify();
	}

	pub fn user_volume(&self, user_id: Snowflake) -> f32 {
		self.local_voice.volumes.get(&user_id).copied().unwrap_or(1.0)
	}

	pub fn is_silenced(&self, user_id: Snowflake) -> bool {
		self.local_voice.silenced.contains(&user_id)
	}

	pub fn set_user_volume(&mut self, user_id: Snowflake, volume: f32, cx: &mut Context<Self>) {
		self.local_voice.volumes.insert(user_id, volume);
		if !self.is_silenced(user_id) {
			self.session.send(Command::SetUserVolume { user_id, volume });
		}
		cx.notify();
	}

	pub fn toggle_silence(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
		let volume = if self.local_voice.silenced.remove(&user_id) {
			self.user_volume(user_id)
		} else {
			self.local_voice.silenced.insert(user_id);
			0.0
		};
		self.session.send(Command::SetUserVolume { user_id, volume });
		cx.notify();
	}

	pub fn search_results(&self) -> Option<Arc<SearchResults>> {
		self.session.search_results()
	}

	pub fn publish(&mut self, event: DataEvent, cx: &mut Context<Self>) {
		cx.emit(event);
	}
}
