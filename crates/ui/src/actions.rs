use gpui::{App, KeyBinding, actions};

pub const SHELL_CONTEXT: &str = "Shell";

actions!(
	oxidcord,
	[
		OpenQuickSwitcher,
		OpenSettings,
		CloseOverlay,
		PreviousChannel,
		NextChannel,
		ZoomIn,
		ZoomOut,
		ZoomReset,
	]
);

pub fn bind_keys(cx: &mut App) {
	cx.bind_keys([
		KeyBinding::new("secondary-k", OpenQuickSwitcher, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-,", OpenSettings, Some(SHELL_CONTEXT)),
		KeyBinding::new("escape", CloseOverlay, Some(SHELL_CONTEXT)),
		KeyBinding::new("alt-up", PreviousChannel, Some(SHELL_CONTEXT)),
		KeyBinding::new("alt-down", NextChannel, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-=", ZoomIn, None),
		KeyBinding::new("secondary-+", ZoomIn, None),
		KeyBinding::new("secondary-shift-=", ZoomIn, None),
		KeyBinding::new("secondary-shift-+", ZoomIn, None),
		KeyBinding::new("secondary--", ZoomOut, None),
		KeyBinding::new("secondary-0", ZoomReset, None),
	]);
}
