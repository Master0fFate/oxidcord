use gpui::App;
use platform::sound::{self, Cue, Looping};
use protocol::StatusKind;
use state::{Changes, State, VoiceCue, VoiceStatus};

use crate::data::{Data, notification_volume, streamer_mode};
use crate::shell::SettingsBackend;

pub struct VoiceSounds {
	seen_cue: u64,
	ring: Option<Looping>,
	ringback: Option<Looping>,
}

fn cue_for(cue: VoiceCue) -> Cue {
	match cue {
		VoiceCue::Join => Cue::Join,
		VoiceCue::Leave => Cue::Leave,
		VoiceCue::Mute => Cue::Mute,
		VoiceCue::Unmute => Cue::Unmute,
		VoiceCue::Deafen => Cue::Deafen,
		VoiceCue::Undeafen => Cue::Undeafen,
		VoiceCue::StreamStart => Cue::StreamStart,
		VoiceCue::StreamEnd => Cue::StreamEnd,
		VoiceCue::CameraOn => Cue::CameraOn,
		VoiceCue::CameraOff => Cue::CameraOff,
	}
}

fn is_ringing_out(state: &State) -> bool {
	state.voice().is_some_and(|voice| {
		voice.guild_id.is_none()
			&& voice.status == VoiceStatus::Connected
			&& !state.ringing_recipients(voice.channel_id).is_empty()
			&& !state.call_has_guests(voice.channel_id)
	})
}

fn is_ringing_in(state: &State) -> bool {
	state.own_status() != StatusKind::Dnd && state.incoming_calls().next().is_some()
}

fn audible_volume(backend: &SettingsBackend, cx: &App) -> Option<f32> {
	((backend.read)().notifications.voice_sounds && !streamer_mode(cx).mutes_sounds())
		.then(|| notification_volume(cx))
}

fn toggle_loop(slot: &mut Option<Looping>, wanted: bool, cue: Cue, volume: impl FnOnce() -> Option<f32>) {
	if !wanted {
		*slot = None;
	} else if slot.is_none()
		&& let Some(volume) = volume()
	{
		*slot = sound::start_loop(cue, volume);
	}
}

impl VoiceSounds {
	pub fn new(state: &State) -> Self {
		Self {
			seen_cue: state.last_voice_cue_id(),
			ring: None,
			ringback: None,
		}
	}

	pub fn update(&mut self, changes: &Changes, data: &Data, backend: &SettingsBackend, cx: &App) {
		if changes.voice_cues {
			self.play_cues(data, backend, cx);
		}
		if changes.calls || changes.voice_session || changes.own_status {
			self.sync_loops(data, backend, cx);
		}
	}

	fn play_cues(&mut self, data: &Data, backend: &SettingsBackend, cx: &App) {
		let latest = {
			let state = data.state();
			let latest = state.voice_cues_after(self.seen_cue).last();
			self.seen_cue = state.last_voice_cue_id();
			latest
		};
		if let Some(cue) = latest
			&& let Some(volume) = audible_volume(backend, cx)
		{
			sound::play(cue_for(cue), volume);
		}
	}

	fn sync_loops(&mut self, data: &Data, backend: &SettingsBackend, cx: &App) {
		let (ringing_in, ringing_out) = {
			let state = data.state();
			(is_ringing_in(&state), is_ringing_out(&state))
		};
		toggle_loop(&mut self.ring, ringing_in, Cue::Ring, || {
			audible_volume(backend, cx)
		});
		toggle_loop(&mut self.ringback, ringing_out, Cue::Ringback, || {
			audible_volume(backend, cx)
		});
	}
}
