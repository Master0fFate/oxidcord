use gpui::{App, Entity};
use protocol::Snowflake;
use session::Command;

use super::quality::Preset;
use super::video_sink::{PreviewSource, Textures};
use crate::data::Data;
use crate::shell::SettingsBackend;
use state::VideoKind;

pub fn join(data: &Entity<Data>, guild: Option<Snowflake>, channel: Snowflake, cx: &App) {
	data.read(cx).send(Command::JoinVoice {
		guild_id: guild,
		channel_id: channel,
	});
}

pub fn leave(data: &Entity<Data>, cx: &App) {
	data.read(cx).send(Command::LeaveVoice);
}

pub fn toggle_camera(data: &Entity<Data>, backend: &SettingsBackend, cx: &mut App) {
	let (camera_on, me) = {
		let data = data.read(cx);
		let on = data.state().voice().is_some_and(|voice| voice.camera_on);
		(on, data.me())
	};
	let textures = cx.try_global::<Textures>().cloned();
	if camera_on {
		data.read(cx).send(Command::StopCamera);
		if let (Some(textures), Some(me)) = (textures, me) {
			textures.0.stop_preview((me, VideoKind::Camera));
		}
		return;
	}
	let voice = (backend.read)().voice;
	data.read(cx).send(Command::StartCamera {
		device_id: voice.camera_device.clone(),
		quality: Preset::from_index(voice.video_preset).quality(None),
	});
	if let (Some(textures), Some(me)) = (textures, me) {
		textures.0.start_preview(
			(me, VideoKind::Camera),
			PreviewSource::Camera(voice.camera_device),
		);
	}
}

pub fn stop_screen_share(data: &Entity<Data>, cx: &mut App) {
	let me = data.read(cx).me();
	data.read(cx).send(Command::StopScreenShare);
	if let (Some(textures), Some(me)) = (cx.try_global::<Textures>().cloned(), me) {
		textures.0.stop_preview((me, VideoKind::Screen));
	}
}
