use std::rc::Rc;

use gpui::{
	Action, AnyElement, App, FontWeight, IntoElement as _, ParentElement as _, SharedString, Styled as _,
	Window, div,
};
use gpui_component::WindowExt as _;
use gpui_component::kbd::Kbd;

use crate::actions::{
	MarkGuildRead, NextChannel, NextGuild, NextMentionChannel, NextUnreadChannel, OpenQuickSwitcher,
	OpenSettings, PreviousChannel, PreviousGuild, PreviousMentionChannel, PreviousUnreadChannel,
	SHELL_CONTEXT, SearchChannel, ShowFriends, ShowShortcuts, ToggleInbox, ToggleSelfDeafen, ToggleSelfMute,
	ZoomIn, ZoomOut, ZoomReset,
};
use crate::i18n::Translate as _;
use crate::theme::{Theme, rem};

const DIALOG_WIDTH: f32 = 440.;

pub fn open(window: &mut Window, cx: &mut App) {
	if window.has_active_dialog(cx) {
		return;
	}
	let rows: [(&str, &[&dyn Action]); 14] = [
		("shortcut-switcher", &[&OpenQuickSwitcher]),
		("shortcut-search", &[&SearchChannel]),
		("shortcut-friends", &[&ShowFriends]),
		("shortcut-inbox", &[&ToggleInbox]),
		("shortcut-mute", &[&ToggleSelfMute]),
		("shortcut-deafen", &[&ToggleSelfDeafen]),
		("shortcut-channel", &[&PreviousChannel, &NextChannel]),
		("shortcut-unread", &[&PreviousUnreadChannel, &NextUnreadChannel]),
		(
			"shortcut-mentions",
			&[&PreviousMentionChannel, &NextMentionChannel],
		),
		("shortcut-server", &[&PreviousGuild, &NextGuild]),
		("shortcut-mark-read", &[&MarkGuildRead]),
		("shortcut-settings", &[&OpenSettings]),
		("shortcut-shortcuts", &[&ShowShortcuts]),
		("shortcut-zoom", &[&ZoomIn, &ZoomOut, &ZoomReset]),
	];
	let resolved: Rc<Vec<(SharedString, Vec<Kbd>)>> = Rc::new(
		rows.iter()
			.map(|(label, actions)| {
				let keys = actions
					.iter()
					.filter_map(|action| Kbd::binding_for_action(*action, Some(SHELL_CONTEXT), window))
					.collect();
				(cx.t(label), keys)
			})
			.collect(),
	);
	window.open_dialog(cx, move |dialog, window, cx| {
		let theme = Theme::current(cx);
		let rows: Vec<AnyElement> = resolved
			.iter()
			.map(|(label, keys)| row(label.clone(), keys, &theme))
			.collect();
		dialog
			.title(cx.t("shortcuts-title"))
			.w(rem(DIALOG_WIDTH).to_pixels(window.rem_size()))
			.child(div().flex().flex_col().gap(theme.space_8).children(rows))
	});
}

fn row(label: SharedString, keys: &[Kbd], theme: &Theme) -> AnyElement {
	div()
		.flex()
		.items_center()
		.justify_between()
		.gap(theme.space_12)
		.text_size(theme.font_sm)
		.text_color(theme.text_normal)
		.child(div().font_weight(FontWeight::MEDIUM).child(label))
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_4)
				.children(keys.iter().cloned()),
		)
		.into_any_element()
}
