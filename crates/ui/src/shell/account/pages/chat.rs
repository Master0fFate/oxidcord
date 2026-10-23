use std::rc::Rc;

use gpui::{AnyElement, App, IntoElement as _, ParentElement as _, Styled as _, div};

use super::widgets::toggle_row;
use crate::i18n::Translate as _;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

pub fn chat_page(backend: &Rc<SettingsBackend>, cx: &App) -> AnyElement {
	let theme = Theme::current(cx);
	let chat = (backend.read)().chat;
	div()
		.flex()
		.flex_col()
		.child(toggle_row(
			"chat-inline-media",
			cx.t("chat-inline-media"),
			Some(cx.t("chat-inline-media-note")),
			chat.inline_media,
			backend,
			|settings, value| settings.chat.inline_media = value,
			&theme,
		))
		.child(toggle_row(
			"chat-link-previews",
			cx.t("chat-link-previews"),
			Some(cx.t("chat-link-previews-note")),
			chat.link_previews,
			backend,
			|settings, value| settings.chat.link_previews = value,
			&theme,
		))
		.child(toggle_row(
			"chat-show-reactions",
			cx.t("chat-show-reactions"),
			Some(cx.t("chat-show-reactions-note")),
			chat.show_reactions,
			backend,
			|settings, value| settings.chat.show_reactions = value,
			&theme,
		))
		.child(toggle_row(
			"chat-time-24h",
			cx.t("chat-time-24h"),
			Some(cx.t("chat-time-24h-note")),
			chat.time_24h,
			backend,
			|settings, value| settings.chat.time_24h = value,
			&theme,
		))
		.child(toggle_row(
			"chat-emoji-autocomplete",
			cx.t("chat-emoji-autocomplete"),
			Some(cx.t("chat-emoji-autocomplete-note")),
			chat.emoji_autocomplete,
			backend,
			|settings, value| settings.chat.emoji_autocomplete = value,
			&theme,
		))
		.into_any_element()
}
