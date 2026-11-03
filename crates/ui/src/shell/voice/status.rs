use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::Snowflake;
use session::Command;

use crate::components::{dialog_footer, field};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::community::data_handle;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const STATUS_LIMIT: usize = 500;

pub struct VoiceStatusEditor {
	data: Entity<Data>,
	channel: Snowflake,
	status: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for VoiceStatusEditor {}

pub fn open(window: &mut Window, cx: &mut App, channel: Snowflake) {
	let data = data_handle(cx);
	let current = data
		.read(cx)
		.state()
		.channel(channel)
		.and_then(|found| found.voice_status().map(str::to_owned))
		.unwrap_or_default();
	let view = cx.new(|cx| VoiceStatusEditor::new(data, channel, current, window, cx));
	let focus = view.read(cx).status.read(cx).focus_handle(cx);
	let title = cx.t("voice-status-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl VoiceStatusEditor {
	fn new(
		data: Entity<Data>,
		channel: Snowflake,
		current: String,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let placeholder = cx.t("voice-status-placeholder");
		let status = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder);
			state.set_value(current, window, cx);
			state
		});
		let subscription = cx.subscribe_in(&status, window, |this, _, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::PressEnter { .. }) {
				this.submit(cx);
			}
		});
		Self {
			data,
			channel,
			status,
			_subscription: subscription,
		}
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		let status: String = self
			.status
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(STATUS_LIMIT)
			.collect();
		self.data.read(cx).send(Command::SetVoiceStatus {
			channel_id: self.channel,
			status: (!status.is_empty()).then_some(status),
		});
		cx.emit(Close);
	}
}

impl Render for VoiceStatusEditor {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let weak_cancel = cx.weak_entity();
		let weak_save = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(
				cx.t("voice-status-title"),
				&theme,
				Input::new(&self.status),
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("settings-save"),
				false,
				true,
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
