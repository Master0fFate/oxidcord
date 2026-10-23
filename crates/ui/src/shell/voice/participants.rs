use gpui::SharedString;
use protocol::{Snowflake, VoiceState};
use state::{State, VideoKind};

use crate::lookup::avatar_for;

#[derive(Clone)]
pub struct Participant {
	pub user: Snowflake,
	pub name: SharedString,
	pub avatar: String,
	pub speaking: bool,
	pub muted: bool,
	pub deafened: bool,
	pub video: Option<VideoKind>,
	pub live: bool,
	pub me: bool,
	pub listener: bool,
	pub hand_raised: bool,
}

fn base(state: &State, guild: Option<Snowflake>, user: Snowflake, avatar_size: u32) -> Participant {
	Participant {
		user,
		name: state.display_name(guild, user).into(),
		avatar: avatar_for(state, user, avatar_size),
		speaking: false,
		muted: false,
		deafened: false,
		video: None,
		live: false,
		me: state.current_user_id() == Some(user),
		listener: false,
		hand_raised: false,
	}
}

fn from_voice_state(
	state: &State,
	guild: Option<Snowflake>,
	voice: &VoiceState,
	avatar_size: u32,
) -> Participant {
	let mut participant = base(state, guild, voice.user_id, avatar_size);
	participant.muted = voice.mute || voice.self_mute;
	participant.deafened = voice.deaf || voice.self_deaf;
	participant.video = if voice.self_video {
		Some(VideoKind::Camera)
	} else if voice.self_stream == Some(true) {
		Some(VideoKind::Screen)
	} else {
		None
	};
	participant.live = voice.self_stream == Some(true);
	participant.listener = voice.suppress;
	participant.hand_raised = voice.request_to_speak_timestamp.is_some();
	participant
}

pub fn participants(
	state: &State,
	guild: Option<Snowflake>,
	channel: Snowflake,
	avatar_size: u32,
) -> Vec<Participant> {
	let mut rows: Vec<Participant> = match guild.and_then(|id| state.guild(id)) {
		Some(guild_state) => guild_state
			.voice_states
			.values()
			.filter(|voice| voice.channel_id == Some(channel))
			.map(|voice| from_voice_state(state, guild, voice, avatar_size))
			.collect(),
		None => state
			.call(channel)
			.map(|call| {
				call.participants
					.iter()
					.map(|user| base(state, guild, *user, avatar_size))
					.collect()
			})
			.unwrap_or_default(),
	};
	let session = state.voice().filter(|session| session.channel_id == channel);
	if let (Some(me), Some(_)) = (state.current_user_id(), session)
		&& !rows.iter().any(|row| row.user == me)
	{
		rows.push(base(state, guild, me, avatar_size));
	}
	if let Some(session) = session {
		for row in &mut rows {
			row.speaking = session.speaking.contains(&row.user);
			if row.me {
				row.muted |= session.self_mute || session.self_deaf;
				row.deafened |= session.self_deaf;
				row.video = if session.camera_on {
					Some(VideoKind::Camera)
				} else if session.screen_share.is_some() {
					Some(VideoKind::Screen)
				} else {
					row.video
				};
			} else if let Some(kind) = session.remote_video.get(&row.user) {
				row.video = Some(*kind);
			}
		}
	}
	for row in &mut rows {
		row.live |= state
			.stream_of(row.user)
			.is_some_and(|stream| stream.channel_id == channel);
	}
	rows.sort_by_cached_key(|row| row.name.to_lowercase());
	rows
}
