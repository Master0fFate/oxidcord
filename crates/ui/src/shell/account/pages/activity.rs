use std::rc::Rc;

use gpui::{
	AnyElement, App, Entity, IntoElement, ParentElement as _, SharedString, Styled as _, div,
	prelude::FluentBuilder as _,
};
use gpui_component::switch::Switch;
use session::Command;
use store::Settings;

use super::widgets::{heading, row};
use crate::components::{PickHandler, select};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

const IDLE_CHOICES_SECS: [u32; 6] = [0, 60, 300, 600, 900, 1800];

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

fn idle_row(backend: &Rc<SettingsBackend>, theme: &Theme, cx: &App) -> AnyElement {
	let current = (backend.read)().presence.idle_after_secs;
	let labels: Vec<SharedString> = IDLE_CHOICES_SECS
		.iter()
		.map(|secs| match secs {
			0 => cx.t("idle-never"),
			secs => cx.t_args("time-minutes", &[("count", i64::from(secs / 60).into())]),
		})
		.collect();
	let selected = IDLE_CHOICES_SECS
		.iter()
		.position(|secs| *secs == current)
		.unwrap_or_default();
	let backend = backend.clone();
	let pick: PickHandler = Rc::new(move |index, _, cx| {
		let secs = IDLE_CHOICES_SECS[index];
		(backend.write)(&|settings| settings.presence.idle_after_secs = secs);
		cx.refresh_windows();
	});
	row(
		cx.t("idle-after"),
		Some(cx.t("idle-after-note")),
		select("idle-after", labels[selected].clone(), &labels, selected, pick),
		theme,
	)
}

pub fn activity_page(backend: &Rc<SettingsBackend>, data: &Entity<Data>, cx: &App) -> AnyElement {
	let theme = Theme::current(cx);
	let local = (backend.read)().activity;
	let show_current_game = data.read(cx).state().show_current_game();
	let shared = data.clone();
	let share_switch = Switch::new("activity-share")
		.checked(show_current_game)
		.on_change(move |checked, _, cx| {
			shared.read(cx).send(Command::SetShowCurrentGame(*checked));
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
		.when(platform::idle::seconds_since_input().is_some(), |page| {
			page.child(idle_row(backend, &theme, cx))
		})
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
