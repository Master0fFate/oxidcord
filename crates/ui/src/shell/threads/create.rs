use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement,
	ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::Snowflake;
use session::Command;

use crate::components::{PickHandler, dialog_footer, field, select};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const NAME_LIMIT: usize = 100;
pub(super) const ARCHIVE_MINUTES: [u32; 4] = [60, 1440, 4320, 10080];
pub(super) const ARCHIVE_KEYS: [&str; 4] = [
	"thread-archive-hour",
	"thread-archive-day",
	"thread-archive-three-days",
	"thread-archive-week",
];
const DEFAULT_ARCHIVE: usize = 1;

pub struct CreateThread {
	data: Entity<Data>,
	channel: Snowflake,
	message: Option<Snowflake>,
	preview: Option<SharedString>,
	name: Entity<InputState>,
	archive: usize,
	_subscription: Subscription,
}

impl EventEmitter<Close> for CreateThread {}

pub fn open(
	window: &mut Window,
	cx: &mut App,
	data: Entity<Data>,
	channel: Snowflake,
	message: Option<Snowflake>,
) {
	let preview = message.and_then(|id| {
		let data = data.read(cx);
		let state = data.state();
		state
			.messages(channel)
			.and_then(|cache| cache.by_id.get(&id))
			.map(|found| {
				let content: String = found.content.chars().take(NAME_LIMIT).collect();
				SharedString::from(content)
			})
	});
	let initial = preview.clone().unwrap_or_default();
	let view = cx.new(|cx| CreateThread::new(data, channel, message, preview, &initial, window, cx));
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t("thread-create-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl CreateThread {
	fn new(
		data: Entity<Data>,
		channel: Snowflake,
		message: Option<Snowflake>,
		preview: Option<SharedString>,
		initial: &str,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let placeholder = cx.t("thread-name-placeholder");
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder);
			state.set_value(initial.to_owned(), window, cx);
			state
		});
		let subscription =
			cx.subscribe_in(
				&name,
				window,
				|this, _, event: &InputEvent, window, cx| match event {
					InputEvent::PressEnter { .. } => this.submit(window, cx),
					InputEvent::Change => cx.notify(),
					_ => {}
				},
			);
		Self {
			data,
			channel,
			message,
			preview,
			name,
			archive: DEFAULT_ARCHIVE,
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

	fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		let name = self.trimmed(cx);
		if name.is_empty() {
			return;
		}
		self.data.read(cx).send(Command::CreateThread {
			channel_id: self.channel,
			message_id: self.message,
			name,
			auto_archive_minutes: ARCHIVE_MINUTES[self.archive],
		});
		cx.emit(Close);
	}
}

impl Render for CreateThread {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels: Vec<SharedString> = ARCHIVE_KEYS.iter().map(|key| cx.t(key)).collect();
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.archive = index;
				cx.notify();
			})
			.ok();
		});
		let enabled = !self.trimmed(cx).is_empty();
		let starter: Option<AnyElement> = self.preview.as_ref().map(|text| {
			field(
				cx.t("thread-starter"),
				&theme,
				div()
					.px(theme.space_12)
					.py(theme.space_8)
					.rounded(theme.radius_sm)
					.bg(theme.background_secondary)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.line_clamp(3)
					.child(text.clone()),
			)
		});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("thread-name"), &theme, Input::new(&self.name)))
			.children(starter)
			.child(field(
				cx.t("thread-auto-archive"),
				&theme,
				select(
					"thread-archive-select",
					labels[self.archive].clone(),
					&labels,
					self.archive,
					pick,
				),
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("thread-create-confirm"),
				false,
				enabled,
				&theme,
				{
					let weak = cx.weak_entity();
					move |_, cx| {
						weak.update(cx, |_, cx| cx.emit(Close)).ok();
					}
				},
				{
					let weak = cx.weak_entity();
					move |window, cx| {
						weak.update(cx, |this, cx| this.submit(window, cx)).ok();
					}
				},
			))
	}
}
