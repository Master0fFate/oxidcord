use std::rc::Rc;

use gpui::{AnyElement, App, Entity, IntoElement, ParentElement as _, SharedString, Styled as _, div};
use gpui_component::switch::Switch;
use serde_json::{Value, json};
use session::Command;
use store::Settings;

use super::widgets::{heading, row};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

fn local_row(
	key: &'static str,
	checked: bool,
	backend: &Rc<SettingsBackend>,
	data: &Entity<Data>,
	apply: fn(&mut Settings, bool),
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	let backend = backend.clone();
	let data = data.clone();
	let switch = Switch::new(key)
		.checked(checked)
		.on_change(move |checked, _, cx| {
			let checked = *checked;
			(backend.write)(&|settings| apply(settings, checked));
			data.read(cx).send(Command::ActivitySettingsChanged);
			cx.refresh_windows();
		});
	let note: SharedString = cx.t(&format!("{key}-note"));
	row(cx.t(key), Some(note), switch, theme)
}

pub fn activity_page(backend: &Rc<SettingsBackend>, data: &Entity<Data>, cx: &App) -> AnyElement {
	let theme = Theme::current(cx);
	let local = (backend.read)().activity;
	let show_current_game = data
		.read(cx)
		.state()
		.user_settings()
		.get("show_current_game")
		.and_then(Value::as_bool)
		.unwrap_or(true);
	let shared = data.clone();
	let share_switch = Switch::new("activity-share")
		.checked(show_current_game)
		.on_change(move |checked, _, cx| {
			shared.read(cx).send(Command::UpdateUserSettings(
				json!({ "show_current_game": *checked }),
			));
		});
	div()
		.flex()
		.flex_col()
		.gap(theme.space_16)
		.child(row(
			cx.t("activity-share"),
			Some(cx.t("activity-share-note")),
			share_switch,
			&theme,
		))
		.child(heading(cx.t("activity-sources"), &theme))
		.child(local_row(
			"activity-rpc-server",
			local.rpc_server,
			backend,
			data,
			|settings, value| settings.activity.rpc_server = value,
			&theme,
			cx,
		))
		.child(local_row(
			"activity-detect-games",
			local.detect_games,
			backend,
			data,
			|settings, value| settings.activity.detect_games = value,
			&theme,
			cx,
		))
		.child(local_row(
			"activity-spotify",
			local.spotify,
			backend,
			data,
			|settings, value| settings.activity.spotify = value,
			&theme,
			cx,
		))
		.into_any_element()
}
