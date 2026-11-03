use gpui::{App, AppContext as _, Bounds, Entity, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use protocol::Snowflake;

use super::viewer::{StreamViewer, ViewerEvent};
use crate::data::Data;
use crate::zoom;

const WINDOW_WIDTH: f32 = 640.;
const WINDOW_HEIGHT: f32 = 360.;
const WINDOW_MIN_WIDTH: f32 = 240.;
const WINDOW_MIN_HEIGHT: f32 = 135.;

pub fn open(data: &Entity<Data>, user: Snowflake, cx: &mut App) {
	let title = {
		let data = data.read(cx);
		let state = data.state();
		let guild = state.voice().and_then(|voice| voice.guild_id);
		state.display_name(guild, user)
	};
	let options = WindowOptions {
		window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
			None,
			size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
			cx,
		))),
		window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
		titlebar: Some(TitlebarOptions {
			title: Some(title.into()),
			..TitlebarOptions::default()
		}),
		..WindowOptions::default()
	};
	let data = data.clone();
	let opened = cx.open_window(options, move |window, cx| {
		zoom::attach(window, cx);
		let viewer = cx.new(|cx| StreamViewer::new(user, &data, cx));
		window
			.subscribe(&viewer, cx, |_, _: &ViewerEvent, window, _| {
				window.remove_window()
			})
			.detach();
		cx.new(|cx| Root::new(viewer, window, cx))
	});
	if let Err(error) = opened {
		tracing::warn!(%error, "could not open the stream window");
	}
}
