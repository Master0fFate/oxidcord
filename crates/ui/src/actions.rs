use gpui::{App, KeyBinding, actions};

pub const SHELL_CONTEXT: &str = "Shell";
pub const LIGHTBOX_CONTEXT: &str = "Lightbox";

actions!(
	oxidcord,
	[
		OpenQuickSwitcher,
		OpenSettings,
		CloseOverlay,
		PreviousChannel,
		NextChannel,
		PreviousUnreadChannel,
		NextUnreadChannel,
		PreviousMentionChannel,
		NextMentionChannel,
		PreviousGuild,
		NextGuild,
		MarkGuildRead,
		SearchChannel,
		ShowFriends,
		ToggleInbox,
		ToggleSelfMute,
		ToggleSelfDeafen,
		ShowShortcuts,
		LightboxPrevious,
		LightboxNext,
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
		KeyBinding::new("alt-shift-up", PreviousUnreadChannel, Some(SHELL_CONTEXT)),
		KeyBinding::new("alt-shift-down", NextUnreadChannel, Some(SHELL_CONTEXT)),
		KeyBinding::new(
			"secondary-alt-shift-up",
			PreviousMentionChannel,
			Some(SHELL_CONTEXT),
		),
		KeyBinding::new(
			"secondary-alt-shift-down",
			NextMentionChannel,
			Some(SHELL_CONTEXT),
		),
		KeyBinding::new("secondary-alt-up", PreviousGuild, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-alt-down", NextGuild, Some(SHELL_CONTEXT)),
		KeyBinding::new("shift-escape", MarkGuildRead, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-f", SearchChannel, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-1", ShowFriends, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-i", ToggleInbox, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-shift-m", ToggleSelfMute, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-shift-d", ToggleSelfDeafen, Some(SHELL_CONTEXT)),
		KeyBinding::new("secondary-/", ShowShortcuts, Some(SHELL_CONTEXT)),
		KeyBinding::new("left", LightboxPrevious, Some(LIGHTBOX_CONTEXT)),
		KeyBinding::new("right", LightboxNext, Some(LIGHTBOX_CONTEXT)),
		KeyBinding::new("secondary-=", ZoomIn, None),
		KeyBinding::new("secondary-+", ZoomIn, None),
		KeyBinding::new("secondary-shift-=", ZoomIn, None),
		KeyBinding::new("secondary-shift-+", ZoomIn, None),
		KeyBinding::new("secondary--", ZoomOut, None),
		KeyBinding::new("secondary-0", ZoomReset, None),
	]);
}
