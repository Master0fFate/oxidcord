use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement,
	ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use protocol::{FormField, FormFieldKind, JoinRequestStatus, Snowflake};
use serde_json::Value;
use session::Command;

use super::modal::{self, Close};
use crate::components::{PickHandler, ToggleHandler, card, dialog_footer, hint, segmented, switch_row};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::theme::Theme;

const NO_CHOICE: usize = usize::MAX;
const TEXT_LIMIT: usize = 150;
const PARAGRAPH_LIMIT: usize = 1000;

enum Answer {
	Agreed(bool),
	Short(Entity<InputState>),
	Long(Entity<TextareaState>),
	Choice(Option<usize>),
	Skipped,
}

struct FieldView {
	label: SharedString,
	description: Option<SharedString>,
	lines: Vec<SharedString>,
}

pub struct Screening {
	data: Entity<Data>,
	guild_id: Snowflake,
	answers: Vec<Answer>,
	views: Vec<FieldView>,
	description: Option<SharedString>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<Close> for Screening {}

pub fn open(window: &mut Window, cx: &mut App, guild_id: Snowflake) {
	let data = super::data_handle(cx);
	let view = cx.new(|cx| Screening::new(data, guild_id, window, cx));
	let title = cx.t("screening-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, None);
}

fn text_response(text: &str) -> Option<Value> {
	let trimmed = text.trim();
	(!trimmed.is_empty()).then(|| Value::from(trimmed))
}

impl Screening {
	fn new(data: Entity<Data>, guild_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let loads = cx.subscribe_in(&data, window, |this, _, event: &DataEvent, window, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.loaded.contains(this.guild_id)
			{
				this.adopt(window, cx);
			}
		});
		let mut screening = Self {
			data,
			guild_id,
			answers: Vec::new(),
			views: Vec::new(),
			description: None,
			_subscriptions: vec![loads],
		};
		if screening
			.data
			.read(cx)
			.state()
			.member_verification(guild_id)
			.is_some()
		{
			screening.adopt(window, cx);
		} else {
			screening
				.data
				.read(cx)
				.send(Command::LoadMemberVerification { guild_id });
		}
		screening
	}

	fn adopt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if !self.answers.is_empty() {
			return;
		}
		let (kinds, views, description): (Vec<(FormFieldKind, Option<String>)>, Vec<FieldView>, _) = {
			let data = self.data.read(cx);
			let state = data.state();
			let Some(form) = state.member_verification(self.guild_id) else {
				return;
			};
			let kinds = form
				.form_fields
				.iter()
				.map(|field| (field.field_type.clone(), field.placeholder.clone()))
				.collect();
			let views = form
				.form_fields
				.iter()
				.map(|field| FieldView {
					label: field.label.clone().into(),
					description: field.description.clone().map(SharedString::from),
					lines: match field.field_type {
						FormFieldKind::Terms => {
							field.values.iter().cloned().map(SharedString::from).collect()
						}
						FormFieldKind::MultipleChoice => {
							field.choices.iter().cloned().map(SharedString::from).collect()
						}
						_ => Vec::new(),
					},
				})
				.collect();
			(kinds, views, form.description.clone().map(SharedString::from))
		};
		self.answers = kinds
			.into_iter()
			.map(|(kind, placeholder)| self.answer_for(kind, placeholder, window, cx))
			.collect();
		self.views = views;
		self.description = description;
		cx.notify();
	}

	fn answer_for(
		&mut self,
		kind: FormFieldKind,
		placeholder: Option<String>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Answer {
		match kind {
			FormFieldKind::Terms => Answer::Agreed(false),
			FormFieldKind::MultipleChoice => Answer::Choice(None),
			FormFieldKind::TextInput => {
				let input = cx.new(|cx| {
					let state = InputState::new(window, cx);
					match placeholder {
						Some(text) => state.placeholder(text),
						None => state,
					}
				});
				self._subscriptions.push(cx.subscribe_in(
					&input,
					window,
					|_, input, event: &InputEvent, window, cx| {
						if matches!(event, InputEvent::Change) {
							let value = input.read(cx).value();
							if value.chars().count() > TEXT_LIMIT {
								let capped: String = value.chars().take(TEXT_LIMIT).collect();
								input.update(cx, |state, cx| state.set_value(capped, window, cx));
							}
							cx.notify();
						}
					},
				));
				Answer::Short(input)
			}
			FormFieldKind::Paragraph => {
				let input = cx.new(|cx| {
					let state = TextareaState::new(window, cx).auto_grow(3, 6);
					match placeholder {
						Some(text) => state.placeholder(text),
						None => state,
					}
				});
				self._subscriptions.push(cx.subscribe_in(
					&input,
					window,
					|_, input, event: &InputEvent, window, cx| {
						if matches!(event, InputEvent::Change) {
							let value = input.read(cx).value();
							if value.chars().count() > PARAGRAPH_LIMIT {
								let capped: String = value.chars().take(PARAGRAPH_LIMIT).collect();
								input.update(cx, |state, cx| state.set_value(capped, window, cx));
							}
							cx.notify();
						}
					},
				));
				Answer::Long(input)
			}
			FormFieldKind::Unknown(_) => Answer::Skipped,
		}
	}

	fn response(&self, index: usize, cx: &App) -> Option<Value> {
		match self.answers.get(index)? {
			Answer::Agreed(true) => Some(Value::Bool(true)),
			Answer::Agreed(false) | Answer::Skipped => None,
			Answer::Short(input) => text_response(&input.read(cx).value()),
			Answer::Long(input) => text_response(&input.read(cx).value()),
			Answer::Choice(choice) => choice.map(Value::from),
		}
	}

	fn complete(&self, fields: &[FormField], cx: &App) -> bool {
		!self.answers.is_empty()
			&& fields.iter().enumerate().all(|(index, field)| {
				!field.required
					|| matches!(self.answers.get(index), Some(Answer::Skipped))
					|| self.response(index, cx).is_some()
			})
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		let Some(mut form) = self
			.data
			.read(cx)
			.state()
			.member_verification(self.guild_id)
			.cloned()
		else {
			return;
		};
		for (index, field) in form.form_fields.iter_mut().enumerate() {
			field.response = self.response(index, cx);
		}
		self.data.read(cx).send(Command::SubmitMemberVerification {
			guild_id: self.guild_id,
			form,
		});
		cx.emit(Close);
	}

	fn set_answer(&mut self, index: usize, answer: Answer, cx: &mut Context<Self>) {
		if let Some(slot) = self.answers.get_mut(index) {
			*slot = answer;
			cx.notify();
		}
	}

	fn rules(&self, index: usize, view: &FieldView, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		let agreed = matches!(self.answers.get(index), Some(Answer::Agreed(true)));
		let weak = cx.weak_entity();
		let toggle: ToggleHandler = Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| this.set_answer(index, Answer::Agreed(value), cx))
				.ok();
		});
		let rules = card(theme).children(view.lines.iter().enumerate().map(|(number, rule)| {
			div()
				.flex()
				.gap(theme.space_8)
				.text_size(theme.font_md)
				.text_color(theme.text_normal)
				.child(
					div()
						.flex_none()
						.font_weight(FontWeight::BOLD)
						.text_color(theme.header_primary)
						.child(format!("{}.", number + 1)),
				)
				.child(div().flex_1().min_w_0().child(rule.clone()))
		}));
		div()
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.child(field_caption(view, theme))
			.child(rules)
			.child(switch_row(
				("screening-agree", index),
				cx.t("screening-agree"),
				None,
				agreed,
				theme,
				toggle,
			))
			.into_any_element()
	}

	fn choices(&self, index: usize, view: &FieldView, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		let selected = match self.answers.get(index) {
			Some(Answer::Choice(Some(choice))) => *choice,
			_ => NO_CHOICE,
		};
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |choice, _, cx| {
			weak.update(cx, |this, cx| {
				this.set_answer(index, Answer::Choice(Some(choice)), cx)
			})
			.ok();
		});
		field_body(
			view,
			theme,
			segmented("screening-choice", &view.lines, selected, theme, pick),
		)
	}

	fn field_view(
		&self,
		index: usize,
		field: &FormField,
		theme: &Theme,
		cx: &Context<Self>,
	) -> Option<AnyElement> {
		let view = self.views.get(index)?;
		match (&field.field_type, self.answers.get(index)?) {
			(FormFieldKind::Terms, _) => Some(self.rules(index, view, theme, cx)),
			(FormFieldKind::MultipleChoice, _) => Some(self.choices(index, view, theme, cx)),
			(_, Answer::Short(input)) => Some(field_body(view, theme, Input::new(input))),
			(_, Answer::Long(input)) => Some(field_body(view, theme, Textarea::new(input))),
			_ => None,
		}
	}
}

fn field_caption(view: &FieldView, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.gap(theme.space_4)
		.child(
			div()
				.text_size(theme.font_md)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(theme.header_primary)
				.child(view.label.clone()),
		)
		.children(view.description.clone().map(|text| hint(text, theme)))
		.into_any_element()
}

fn field_body(view: &FieldView, theme: &Theme, content: impl IntoElement) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.child(field_caption(view, theme))
		.child(content)
		.into_any_element()
}

impl Screening {
	fn closed(&self, title: &'static str, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let weak_close = cx.weak_entity();
		let weak_withdraw = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(hint(cx.t(title), theme))
			.child(dialog_footer(
				cx.t("action-close"),
				cx.t("screening-withdraw"),
				true,
				true,
				theme,
				move |_, cx| {
					weak_close.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_withdraw
						.update(cx, |this, cx| {
							this.data.read(cx).send(Command::WithdrawJoinRequest {
								guild_id: this.guild_id,
							});
							cx.emit(Close);
						})
						.ok();
				},
			))
			.into_any_element()
	}
}

impl Render for Screening {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let sent = self
			.data
			.read(cx)
			.state()
			.application(self.guild_id)
			.and_then(|application| application.status.clone());
		match sent {
			Some(JoinRequestStatus::Submitted) => {
				return self
					.closed("membership-bar-submitted", &theme, cx)
					.into_any_element();
			}
			Some(JoinRequestStatus::Rejected) => {
				return self
					.closed("membership-bar-rejected", &theme, cx)
					.into_any_element();
			}
			_ => {}
		}
		let data = self.data.clone();
		let data = data.read(cx);
		let state = data.state();
		let form = state.member_verification(self.guild_id);
		let fields: &[FormField] = form.map_or(&[], |form| form.form_fields.as_slice());
		let complete = self.complete(fields, cx);
		let body = div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.children(self.description.clone().map(|text| hint(text, &theme)))
			.children(
				fields
					.iter()
					.enumerate()
					.filter_map(|(index, field)| self.field_view(index, field, &theme, cx)),
			);
		let weak_cancel = cx.weak_entity();
		let weak_confirm = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(body)
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("screening-submit"),
				false,
				complete,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_confirm.update(cx, |this, cx| this.submit(cx)).ok();
				},
			))
			.into_any_element()
	}
}
