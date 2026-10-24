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

pub fn role_color(color: u32) -> Hsla {
	rgb(color).into()
}
