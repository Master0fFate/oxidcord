use std::cell::Cell;

use gpui::{App, Entity, EventEmitter, FocusHandle, ParentElement as _, Rems, Render, SharedString, Window};
use gpui_component::WindowExt as _;

pub struct Close;

pub fn open<V>(
	window: &mut Window,
	cx: &mut App,
	title: impl Into<SharedString>,
	width: Rems,
	view: &Entity<V>,
	focus: Option<FocusHandle>,
) where
	V: Render + EventEmitter<Close>,
{
	window
		.subscribe(view, cx, |_, _: &Close, window, cx| window.close_dialog(cx))
		.detach();
	let view = view.clone();
	let title = title.into();
	let initial = Cell::new(true);
	window.open_dialog(cx, move |dialog, window, cx| {
		if initial.replace(false)
			&& let Some(focus) = &focus
		{
			window.focus(focus, cx);
		}
		dialog
			.w(width.to_pixels(window.rem_size()))
			.title(title.clone())
			.child(view.clone())
	});
}
