use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{Hsla, rgb};
use protocol::StatusKind;
use state::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
	Online,
	Idle,
	Dnd,
	#[default]
	Offline,
}

impl From<&StatusKind> for Status {
	fn from(kind: &StatusKind) -> Self {
		match kind {
			StatusKind::Online => Self::Online,
			StatusKind::Idle => Self::Idle,
			StatusKind::Dnd => Self::Dnd,
			_ => Self::Offline,
		}
	}
}

pub fn status_of(state: &State, user_id: protocol::Snowflake) -> Status {
	if state.current_user_id() == Some(user_id) {
		return Status::from(&state.own_status());
	}
	state
		.presence(user_id)
		.map_or(Status::Offline, |presence| Status::from(&presence.status))
}

static ROLE_SATURATION: AtomicU8 = AtomicU8::new(MAX_ROLE_SATURATION);

pub const MAX_ROLE_SATURATION: u8 = 100;

pub fn set_role_saturation(percent: u8) {
	ROLE_SATURATION.store(percent.min(MAX_ROLE_SATURATION), Ordering::Relaxed);
}

pub fn role_color(color: u32) -> Hsla {
	rgb(color).into()
}

pub fn role_display_color(color: u32) -> Hsla {
	let mut color = role_color(color);
	color.s *= f32::from(ROLE_SATURATION.load(Ordering::Relaxed)) / f32::from(MAX_ROLE_SATURATION);
	color
}
