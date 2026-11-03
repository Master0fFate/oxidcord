use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement,
	ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::{ChannelType, Permissions, Snowflake};
use session::Command;

use super::cooldown;
use crate::components::{PickHandler, ToggleHandler, dialog_footer, field, select, switch_row};
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

#[derive(Clone, Copy)]
struct Options {
	private: bool,
	offer_private: bool,
	moderator: bool,
}

pub struct CreateThread {
	data: Entity<Data>,
	channel: Snowflake,
	message: Option<Snowflake>,
	preview: Option<SharedString>,
	name: Entity<InputState>,
	archive: usize,
	invitable: bool,
	options: Options,
	_cooldown: Option<Task<()>>,
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
	let options = thread_options(&data, channel, message, cx);
	let view = cx.new(|cx| CreateThread::new(data, channel, message, preview, options, window, cx));
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t("thread-create-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn thread_options(data: &Entity<Data>, channel: Snowflake, message: Option<Snowflake>, cx: &App) -> Options {
	let data = data.read(cx);
	let state = data.state();
	let permissions = state.my_permissions(channel);
	let text = state
		.channel(channel)
		.is_some_and(|parent| parent.kind == ChannelType::GuildText);
	let can_private = message.is_none() && text && permissions.contains(Permissions::CREATE_PRIVATE_THREADS);
	let can_public = permissions.contains(Permissions::CREATE_PUBLIC_THREADS);
	Options {
		private: can_private && !can_public,
		offer_private: can_private && can_public,
		moderator: permissions.contains(Permissions::MANAGE_THREADS),
	}
}

impl CreateThread {
	fn new(
		data: Entity<Data>,
		channel: Snowflake,
		message: Option<Snowflake>,
		preview: Option<SharedString>,
		options: Options,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let placeholder = cx.t("thread-name-placeholder");
		let initial = preview.clone().unwrap_or_default();
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder);
			state.set_value(initial, window, cx);
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
			_cooldown: cooldown::watch(data.clone(), channel, cx),
			data,
			channel,
			message,
			preview,
			name,
			archive: DEFAULT_ARCHIVE,
			invitable: true,
			options,
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
		if name.is_empty() || cooldown::remaining(&self.data, self.channel, cx).is_some() {
			return;
		}
		self.data.read(cx).send(Command::CreateThread {
			channel_id: self.channel,
			message_id: self.message,
			name,
			auto_archive_minutes: ARCHIVE_MINUTES[self.archive],
			private: self.options.private,
			invitable: self.invitable,
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
		let weak = cx.weak_entity();
		let private: ToggleHandler = Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				this.options.private = value;
				cx.notify();
			})
			.ok();
		});
		let weak = cx.weak_entity();
		let invitable: ToggleHandler = Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				this.invitable = value;
				cx.notify();
			})
			.ok();
		});
		let waiting = cooldown::remaining(&self.data, self.channel, cx);
		let enabled = !self.trimmed(cx).is_empty() && waiting.is_none();
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
			.when(self.options.offer_private, |form| {
				form.child(switch_row(
					"thread-private",
					cx.t("thread-private"),
					None,
					self.options.private,
					&theme,
					private,
				))
			})
			.when(self.options.private && self.options.moderator, |form| {
				form.child(switch_row(
					"thread-invitable",
					cx.t("thread-invitable"),
					None,
					self.invitable,
					&theme,
					invitable,
				))
			})
			.children(waiting.map(|remaining| cooldown::line(remaining, &theme, cx)))
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
