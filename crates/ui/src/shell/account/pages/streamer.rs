use std::rc::Rc;

use gpui::{AnyElement, App, IntoElement as _, ParentElement as _, Styled as _, div};

use super::widgets::toggle_row;
use crate::i18n::Translate as _;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

pub fn streamer_page(backend: &Rc<SettingsBackend>, cx: &App) -> AnyElement {
	let theme = Theme::current(cx);
	let streamer = (backend.read)().streamer_mode;
	div()
		.flex()
		.flex_col()
		.child(toggle_row(
			"streamer-enabled",
			cx.t("streamer-enabled"),
			Some(cx.t("streamer-enabled-note")),
			streamer.enabled,
			backend,
			|settings, value| settings.streamer_mode.enabled = value,
			&theme,
		))
		.child(toggle_row(
			"streamer-hide-personal",
			cx.t("streamer-hide-personal"),
			Some(cx.t("streamer-hide-personal-note")),
			streamer.hide_personal,
			backend,
			|settings, value| settings.streamer_mode.hide_personal = value,
			&theme,
		))
		.child(toggle_row(
			"streamer-hide-invites",
			cx.t("streamer-hide-invites"),
			Some(cx.t("streamer-hide-invites-note")),
			streamer.hide_invites,
			backend,
			|settings, value| settings.streamer_mode.hide_invites = value,
			&theme,
		))
		.child(toggle_row(
			"streamer-disable-sounds",
			cx.t("streamer-disable-sounds"),
			Some(cx.t("streamer-disable-sounds-note")),
			streamer.disable_sounds,
			backend,
			|settings, value| settings.streamer_mode.disable_sounds = value,
			&theme,
		))
		.into_any_element()
}
