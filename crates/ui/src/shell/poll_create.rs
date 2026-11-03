use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, InteractiveElement as _,
	IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::{PartialEmoji, PollAnswer, PollCreate, PollLayout, PollMedia, Snowflake};
use session::Command;

use crate::components::{
	IconButton, PickHandler, StatusEmoji, ToggleHandler, dialog_footer, field, select, switch_row,
};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::community::modal::{self, Close};
use crate::shell::emoji_picker::{EmojiPicker, EmojiPickerEvent};
use crate::theme::Theme;

const QUESTION_LIMIT: usize = 300;
const ANSWER_LIMIT: usize = 55;
const MIN_ANSWERS: usize = 2;
const MAX_ANSWERS: usize = 10;
const DURATION_HOURS: [u32; 7] = [1, 4, 8, 24, 72, 168, 336];
const DEFAULT_DURATION: usize = 3;

struct Answer {
	input: Entity<InputState>,
	emoji: Option<PartialEmoji>,
	_subscription: Subscription,
}

pub struct PollForm {
	data: Entity<Data>,
	channel: Snowflake,
	question: Entity<InputState>,
	answers: Vec<Answer>,
	duration: usize,
	multiselect: bool,
	picker: Entity<EmojiPicker>,
	picking: Option<usize>,
	_subscriptions: [Subscription; 2],
}

impl EventEmitter<Close> for PollForm {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, channel: Snowflake) {
	let view = cx.new(|cx| PollForm::new(data, channel, window, cx));
	let focus = view.read(cx).question.read(cx).focus_handle(cx);
	let title = cx.t("poll-create-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn trimmed(input: &Entity<InputState>, limit: usize, cx: &App) -> String {
	input.read(cx).value().trim().chars().take(limit).collect()
}

impl PollForm {
	fn new(data: Entity<Data>, channel: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let placeholder = cx.t("poll-question-placeholder");
		let question = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let question_subscription = cx.subscribe(&question, |_, _, event: &InputEvent, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		let picker = cx.new(|cx| EmojiPicker::new(window, cx));
		picker.update(cx, |picker, cx| picker.open(Vec::new(), window, cx));
		let picker_subscription = cx.subscribe(&picker, |this, _, event: &EmojiPickerEvent, cx| {
			let EmojiPickerEvent::Pick(choice) = event;
			if let Some(answer) = this.picking.take().and_then(|index| this.answers.get_mut(index)) {
				answer.emoji = Some(choice.partial());
			}
			cx.notify();
		});
		let mut form = Self {
			data,
			channel,
			question,
			answers: Vec::with_capacity(MAX_ANSWERS),
			duration: DEFAULT_DURATION,
			multiselect: false,
			picker,
			picking: None,
			_subscriptions: [question_subscription, picker_subscription],
		};
		for _ in 0..MIN_ANSWERS {
			form.push_answer(window, cx);
		}
		form
	}

	fn push_answer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let placeholder = cx.t("poll-answer-placeholder");
		let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscription = cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
			if matches!(event, InputEvent::Change) {
				cx.notify();
			}
		});
		self.answers.push(Answer {
			input,
			emoji: None,
			_subscription: subscription,
		});
		cx.notify();
	}

	fn remove_answer(&mut self, index: usize, cx: &mut Context<Self>) {
		self.answers.remove(index);
		self.picking = None;
		cx.notify();
	}

	fn toggle_picker(&mut self, index: usize, cx: &mut Context<Self>) {
		self.picking = (self.picking != Some(index)).then_some(index);
		cx.notify();
	}

	fn filled_answers(&self, cx: &App) -> Vec<PollAnswer> {
		self.answers
			.iter()
			.filter_map(|answer| {
				let text = trimmed(&answer.input, ANSWER_LIMIT, cx);
				(!text.is_empty()).then(|| PollAnswer {
					answer_id: None,
					poll_media: PollMedia {
						text: Some(text),
						emoji: answer.emoji.clone(),
					},
				})
			})
			.collect()
	}

	fn ready(&self, cx: &App) -> bool {
		!trimmed(&self.question, QUESTION_LIMIT, cx).is_empty()
			&& self.filled_answers(cx).len() >= MIN_ANSWERS
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if !self.ready(cx) {
			return;
		}
		let poll = PollCreate {
			question: PollMedia {
				text: Some(trimmed(&self.question, QUESTION_LIMIT, cx)),
				emoji: None,
			},
			answers: self.filled_answers(cx),
			duration: DURATION_HOURS[self.duration],
			allow_multiselect: self.multiselect,
			layout_type: PollLayout::Default,
		};
		self.data.read(cx).send(Command::SendPoll {
			channel_id: self.channel,
			poll: Box::new(poll),
		});
		cx.emit(Close);
	}

	fn answer_row(
		&self,
		index: usize,
		answer: &Answer,
		removable: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let chosen = answer.emoji.as_ref().and_then(StatusEmoji::from_activity);
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.child(
				div()
					.id(("poll-answer-emoji", index))
					.flex()
					.flex_none()
					.items_center()
					.justify_center()
					.size(theme.field_height)
					.rounded(theme.radius_sm)
					.bg(theme.input_background)
					.cursor_pointer()
					.hover(|style| style.bg(theme.modifier_selected))
					.on_click(cx.listener(move |this, _, _, cx| this.toggle_picker(index, cx)))
					.child(match chosen {
						Some(emoji) => emoji.element(theme.icon_lg, cx),
						None => Icon::new(IconKind::Emoji)
							.size(theme.icon_lg)
							.color(theme.interactive_normal)
							.into_any_element(),
					}),
			)
			.child(div().flex_1().child(Input::new(&answer.input)))
			.when(removable, |row| {
				row.child(
					IconButton::new(("poll-answer-remove", index), IconKind::Close)
						.tooltip(cx.t("poll-remove-answer"))
						.on_click(cx.listener(move |this, _, _, cx| this.remove_answer(index, cx))),
				)
			})
			.into_any_element()
	}
}

impl Render for PollForm {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let removable = self.answers.len() > MIN_ANSWERS;
		let rows: Vec<AnyElement> = self
			.answers
			.iter()
			.enumerate()
			.map(|(index, answer)| self.answer_row(index, answer, removable, &theme, cx))
			.collect();
		let labels: Vec<SharedString> = DURATION_HOURS
			.iter()
			.map(|hours| cx.t(&format!("poll-duration-{hours}")))
			.collect();
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.duration = index;
				cx.notify();
			})
			.ok();
		});
		let weak = cx.weak_entity();
		let toggle: ToggleHandler = Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				this.multiselect = value;
				cx.notify();
			})
			.ok();
		});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("poll-question"), &theme, Input::new(&self.question)))
			.child(field(
				cx.t("poll-answers"),
				&theme,
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.children(rows)
					.when(self.picking.is_some(), |list| list.child(self.picker.clone()))
					.when(self.answers.len() < MAX_ANSWERS, |list| {
						list.child(
							div().flex().child(
								Button::new("poll-add-answer")
									.small()
									.outline()
									.label(cx.t("poll-add-answer"))
									.on_click(
										cx.listener(|this, _, window, cx| this.push_answer(window, cx)),
									),
							),
						)
					}),
			))
			.child(field(
				cx.t("poll-duration"),
				&theme,
				select(
					"poll-duration-select",
					labels[self.duration].clone(),
					&labels,
					self.duration,
					pick,
				),
			))
			.child(switch_row(
				"poll-multiselect",
				cx.t("poll-multiselect"),
				None,
				self.multiselect,
				&theme,
				toggle,
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("poll-create-confirm"),
				false,
				self.ready(cx),
				&theme,
				{
					let weak = cx.weak_entity();
					move |_, cx| {
						weak.update(cx, |_, cx| cx.emit(Close)).ok();
					}
				},
				{
					let weak = cx.weak_entity();
					move |_, cx| {
						weak.update(cx, |this, cx| this.submit(cx)).ok();
					}
				},
			))
	}
}
