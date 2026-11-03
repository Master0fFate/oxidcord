use std::collections::HashMap;

use gpui::{
	AnyElement, IntoElement as _, ParentElement as _, SharedString, Styled as _, div,
	prelude::FluentBuilder as _,
};
use media::SizeHint;
use protocol::Snowflake;
use state::State;

use crate::components::Avatar;
use crate::lookup::avatar_for;
use crate::theme::Theme;

pub const PREVIEW_HINT: SizeHint = SizeHint::new(512, 288);

const MAX_FACES: usize = 3;
const FACE_AVATAR_PIXELS: u32 = 32;

pub struct Face {
	name: SharedString,
	avatar: SharedString,
}

pub struct LiveStream {
	pub key: String,
	pub preview: Option<SharedString>,
	viewers: usize,
	faces: Vec<Face>,
}

pub fn live_streams(
	state: &State,
	guild: Option<Snowflake>,
	channel: Snowflake,
) -> HashMap<Snowflake, LiveStream> {
	let mut live: HashMap<Snowflake, LiveStream> = state
		.streams()
		.filter(|stream| stream.channel_id == channel)
		.map(|stream| {
			let faces = stream
				.viewers
				.iter()
				.take(MAX_FACES)
				.map(|viewer| Face {
					name: state.display_name(guild, *viewer).into(),
					avatar: avatar_for(state, *viewer, FACE_AVATAR_PIXELS).into(),
				})
				.collect();
			(
				stream.user_id,
				LiveStream {
					key: stream.stream_key.clone(),
					preview: stream.preview_url.as_deref().map(SharedString::from),
					viewers: stream.viewers.len(),
					faces,
				},
			)
		})
		.collect();
	if let Some(guild) = guild
		&& let Some(found) = state.guild(guild)
	{
		for voice in found
			.voice_states
			.values()
			.filter(|voice| voice.channel_id == Some(channel) && voice.self_stream == Some(true))
		{
			live.entry(voice.user_id).or_insert_with(|| LiveStream {
				key: format!("guild:{guild}:{channel}:{}", voice.user_id),
				preview: None,
				viewers: 0,
				faces: Vec::new(),
			});
		}
	}
	live
}

pub fn viewer_stack(stream: &LiveStream, theme: &Theme) -> Option<AnyElement> {
	if stream.viewers == 0 {
		return None;
	}
	Some(
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_4)
			.px(theme.space_4)
			.py(theme.space_4 * 0.5)
			.rounded(theme.radius_sm)
			.bg(theme.overlay)
			.child(
				div()
					.flex()
					.items_center()
					.children(stream.faces.iter().enumerate().map(|(index, face)| {
						div().when(index > 0, |slot| slot.ml(-theme.space_4)).child(
							Avatar::new(face.name.clone())
								.size(theme.voice_icon_sm)
								.url(face.avatar.clone())
								.cutout(theme.overlay),
						)
					})),
			)
			.child(
				div()
					.text_size(theme.font_xs)
					.text_color(theme.interactive_active)
					.child(stream.viewers.to_string()),
			)
			.into_any_element(),
	)
}
