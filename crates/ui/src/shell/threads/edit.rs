use std::rc::Rc;

use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::Snowflake;
use session::Command;

use super::create::{ARCHIVE_KEYS, ARCHIVE_MINUTES};
use crate::components::{PickHandler, dialog_footer, field, select};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::community::modal::{self, Close};
use crate::shell::server_settings::channel::SLOWMODES;
use crate::theme::Theme;

const NAME_LIMIT: usize = 100;

pub struct EditThread {
	data: Entity<Data>,
	thread: Snowflake,
	name: Entity<InputState>,
	slowmode: usize,
	archive: usize,
	_subscription: Subscription,
}

impl EventEmitter<Close> for EditThread {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, thread: Snowflake, post: bool) {
	let view = cx.new(|cx| EditThread::new(data, thread, window, cx));
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t(if post { "forum-edit-post" } else { "thread-edit" });
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn closest(values: impl Iterator<Item = u32>, wanted: u32) -> usize {
	values
		.enumerate()
		.min_by_key(|(_, value)| value.abs_diff(wanted))
		.map_or(0, |(index, _)| index)
}

impl EditThread {
	fn new(data: Entity<Data>, thread: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let (name, slowmode, archive) = {
			let data = data.read(cx);
			let state = data.state();
			let channel = state.channel(thread);
			(
				channel
					.and_then(|channel| channel.name.clone())
					.unwrap_or_default(),
				channel
					.and_then(|channel| channel.rate_limit_per_user)
					.and_then(|seconds| u32::try_from(seconds).ok())
					.unwrap_or(0),
				channel
					.and_then(|channel| channel.thread_metadata.as_ref()?.auto_archive_duration)
					.and_then(|minutes| u32::try_from(minutes).ok())
					.unwrap_or(ARCHIVE_MINUTES[1]),
			)
		};
		let placeholder = cx.t("thread-name-placeholder");
		let input = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder);
			state.set_value(name, window, cx);
			state
		});
		let subscription =
			cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| match event {
				InputEvent::PressEnter { .. } => this.submit(cx),
				InputEvent::Change => cx.notify(),
				_ => {}
			});
		Self {
			data,
			thread,
			name: input,
			slowmode: closest(SLOWMODES.iter().map(|(_, seconds)| *seconds), slowmode),
			archive: closest(ARCHIVE_MINUTES.into_iter(), archive),
			_subscription: subscription,
		}
	}

	fn trimmed(&self, cx: &App) -> String {
		self.name
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NAME_LIMIT)
			.collect()
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		let name = self.trimmed(cx);
		if name.is_empty() {
			return;
		}
		self.data.read(cx).send(Command::EditThread {
			thread_id: self.thread,
			name,
			slowmode: SLOWMODES[self.slowmode].1,
			auto_archive_minutes: ARCHIVE_MINUTES[self.archive],
		});
		cx.emit(Close);
	}

	fn picker(&self, cx: &Context<Self>, apply: fn(&mut Self, usize)) -> PickHandler {
		let weak = cx.weak_entity();
		Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				apply(this, index);
				cx.notify();
			})
			.ok();
		})
	}
}

impl Render for EditThread {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let slowmodes: Vec<SharedString> = SLOWMODES.iter().map(|(key, _)| cx.t(key)).collect();
		let archives: Vec<SharedString> = ARCHIVE_KEYS.iter().map(|key| cx.t(key)).collect();
		let pick_slowmode = self.picker(cx, |this, index| this.slowmode = index);
		let pick_archive = self.picker(cx, |this, index| this.archive = index);
		let enabled = !self.trimmed(cx).is_empty();
		let weak_cancel = cx.weak_entity();
		let weak_save = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("thread-name"), &theme, Input::new(&self.name)))
			.child(field(
				cx.t("channel-slowmode"),
				&theme,
				select(
					"thread-slowmode-select",
					slowmodes[self.slowmode].clone(),
					&slowmodes,
					self.slowmode,
					pick_slowmode,
				),
			))
			.child(field(
				cx.t("thread-auto-archive"),
				&theme,
				select(
					"thread-archive-select",
					archives[self.archive].clone(),
					&archives,
					self.archive,
					pick_archive,
				),
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("settings-save"),
				false,
				enabled,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_save.update(cx, |this, cx| this.submit(cx)).ok();
				},
			))
	}
}
