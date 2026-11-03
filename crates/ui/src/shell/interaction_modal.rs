use std::rc::Rc;
use std::sync::Arc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement,
	ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use protocol::rest::{ModalAnswer, ModalValue};
use protocol::{Component, ComponentType, InteractionModal, SelectOption, TextInputStyle};
use session::{Command, ModalSubmit};

use crate::components::{
	PickHandler, ToggleHandler, dialog_footer, field, hint, segmented, select, switch_row,
};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const TEXT_LIMIT: usize = 4000;

enum TextBox {
	Short(Entity<InputState>),
	Paragraph(Entity<TextareaState>),
}

impl TextBox {
	fn value(&self, cx: &App) -> String {
		match self {
			Self::Short(input) => input.read(cx).value().to_string(),
			Self::Paragraph(input) => input.read(cx).value().to_string(),
		}
	}
}

struct Choices {
	options: Vec<SelectOption>,
	picked: Vec<bool>,
	min: usize,
	max: usize,
}

enum Control {
	Text { input: TextBox, min: usize, max: usize },
	Dropdown(Choices),
	Radio(Choices),
	Toggles(Choices),
	Flag(bool),
}

struct Item {
	label: SharedString,
	description: Option<SharedString>,
	required: bool,
	kind: ComponentType,
	custom_id: String,
	component_id: Option<i64>,
	wrapper: Option<(ComponentType, Option<i64>)>,
	control: Control,
}

pub struct ModalForm {
	data: Entity<Data>,
	modal: Arc<InteractionModal>,
	items: Vec<Item>,
	notes: Vec<SharedString>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<Close> for ModalForm {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, interaction: Arc<InteractionModal>) {
	let title = SharedString::from(interaction.title.clone());
	let view = cx.new(|cx| ModalForm::new(data, interaction, window, cx));
	let focus = view.read(cx).first_focus(cx);
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, focus);
}

fn choices(component: &Component, radio: bool) -> Choices {
	let options: Vec<SelectOption> = component.options.iter().cloned().collect();
	let picked = options.iter().map(|option| option.default).collect();
	let single =
		radio || (component.kind == ComponentType::StringSelect && component.max_values.unwrap_or(1) <= 1);
	let default_min = usize::from(component.is_required());
	let min = component.min_values.map_or(default_min, |min| min as usize);
	let max = if single {
		1
	} else {
		component
			.max_values
			.map_or(options.len(), |max| max as usize)
			.min(options.len())
	};
	Choices {
		options,
		picked,
		min: if radio { default_min } else { min },
		max,
	}
}

impl ModalForm {
	fn new(
		data: Entity<Data>,
		modal: Arc<InteractionModal>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let mut form = Self {
			data,
			modal,
			items: Vec::new(),
			notes: Vec::new(),
			_subscriptions: Vec::new(),
		};
		let modal = Arc::clone(&form.modal);
		for component in modal.components.iter() {
			form.add(component, window, cx);
		}
		form
	}

	fn add(&mut self, component: &Component, window: &mut Window, cx: &mut Context<Self>) {
		match component.kind {
			ComponentType::Label => {
				if let Some(inner) = component.component.as_deref() {
					let wrapper = Some((ComponentType::Label, component.id));
					let label = component.label.clone().unwrap_or_default();
					self.add_control(inner, label, component.description.clone(), wrapper, window, cx);
				}
			}
			ComponentType::ActionRow => {
				let wrapper = Some((ComponentType::ActionRow, component.id));
				for inner in component.components.iter() {
					let label = inner.label.clone().unwrap_or_default();
					self.add_control(inner, label, None, wrapper, window, cx);
				}
			}
			ComponentType::TextDisplay => {
				if let Some(content) = &component.content {
					self.notes.push(content.clone().into());
				}
			}
			_ => {}
		}
	}

	fn add_control(
		&mut self,
		component: &Component,
		label: String,
		description: Option<String>,
		wrapper: Option<(ComponentType, Option<i64>)>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let Some(custom_id) = component.custom_id.clone() else {
			return;
		};
		let control = match component.kind {
			ComponentType::TextInput => self.text_control(component, window, cx),
			ComponentType::StringSelect => {
				let picks = choices(component, false);
				if picks.max <= 1 {
					Control::Dropdown(picks)
				} else {
					Control::Toggles(picks)
				}
			}
			ComponentType::RadioGroup => Control::Radio(choices(component, true)),
			ComponentType::CheckboxGroup => Control::Toggles(choices(component, false)),
			ComponentType::Checkbox => Control::Flag(component.default),
			_ => return,
		};
		self.items.push(Item {
			label: label.into(),
			description: description.map(Into::into),
			required: component.is_required(),
			kind: component.kind,
			custom_id,
			component_id: component.id,
			wrapper,
			control,
		});
	}

	fn text_control(
		&mut self,
		component: &Component,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Control {
		let placeholder = component.placeholder.clone().unwrap_or_default();
		let prefill = component.value.clone().unwrap_or_default();
		let input = match component.text_input_style() {
			TextInputStyle::Paragraph => {
				let state = cx.new(|cx| {
					let mut state = TextareaState::new(window, cx)
						.placeholder(placeholder)
						.auto_grow(3, 8);
					state.set_value(prefill, window, cx);
					state
				});
				self.watch(&state, cx);
				TextBox::Paragraph(state)
			}
			_ => {
				let state = cx.new(|cx| {
					let mut state = InputState::new(window, cx).placeholder(placeholder);
					state.set_value(prefill, window, cx);
					state
				});
				self.watch(&state, cx);
				TextBox::Short(state)
			}
		};
		Control::Text {
			input,
			min: component.min_length.map_or(0, |min| min as usize),
			max: component.max_length.map_or(TEXT_LIMIT, |max| max as usize),
		}
	}

	fn watch<T>(&mut self, state: &Entity<T>, cx: &mut Context<Self>)
	where
		T: EventEmitter<InputEvent> + 'static,
	{
		self._subscriptions
			.push(cx.subscribe(state, |_, _, event: &InputEvent, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}));
	}

	fn first_focus(&self, cx: &App) -> Option<gpui::FocusHandle> {
		self.items.iter().find_map(|item| match &item.control {
			Control::Text {
				input: TextBox::Short(input),
				..
			} => Some(input.read(cx).focus_handle(cx)),
			Control::Text {
				input: TextBox::Paragraph(input),
				..
			} => Some(input.read(cx).focus_handle(cx)),
			_ => None,
		})
	}

	fn item_ready(item: &Item, cx: &App) -> bool {
		match &item.control {
			Control::Text { input, min, max } => {
				let length = input.value(cx).trim().chars().count();
				(length == 0 && !item.required)
					|| (length >= (*min).max(usize::from(item.required)) && length <= *max)
			}
			Control::Dropdown(picks) | Control::Radio(picks) | Control::Toggles(picks) => {
				let count = picks.picked.iter().filter(|picked| **picked).count();
				count >= picks.min && count <= picks.max
			}
			Control::Flag(_) => true,
		}
	}

	fn ready(&self, cx: &App) -> bool {
		self.items.iter().all(|item| Self::item_ready(item, cx))
	}

	fn answer(item: &Item, cx: &App) -> ModalAnswer {
		let mut answer = ModalAnswer {
			custom_id: Some(item.custom_id.clone()),
			..ModalAnswer::new(item.kind, item.component_id)
		};
		let chosen = |picks: &Choices| -> Vec<String> {
			picks
				.options
				.iter()
				.zip(&picks.picked)
				.filter(|(_, picked)| **picked)
				.map(|(option, _)| option.value.clone())
				.collect()
		};
		match &item.control {
			Control::Text { input, .. } => answer.value = Some(ModalValue::Text(Some(input.value(cx)))),
			Control::Dropdown(picks) | Control::Toggles(picks) => answer.values = Some(chosen(picks)),
			Control::Radio(picks) => answer.value = Some(ModalValue::Text(chosen(picks).into_iter().next())),
			Control::Flag(flag) => answer.value = Some(ModalValue::Flag(*flag)),
		}
		answer
	}

	fn wrapped(item: &Item, cx: &App) -> ModalAnswer {
		let answer = Self::answer(item, cx);
		match item.wrapper {
			Some((ComponentType::ActionRow, id)) => ModalAnswer {
				components: vec![answer],
				..ModalAnswer::new(ComponentType::ActionRow, id)
			},
			Some((kind, id)) => ModalAnswer {
				component: Some(Box::new(answer)),
				..ModalAnswer::new(kind, id)
			},
			None => answer,
		}
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if !self.ready(cx) {
			return;
		}
		let components = self.items.iter().map(|item| Self::wrapped(item, cx)).collect();
		self.data.read(cx).send(Command::SubmitModal(ModalSubmit {
			interaction_id: self.modal.id,
			application_id: self.modal.application.id,
			channel_id: self.modal.channel_id,
			custom_id: self.modal.custom_id.clone(),
			components,
		}));
		cx.emit(Close);
	}

	fn pick(&mut self, index: usize, choice: usize, cx: &mut Context<Self>) {
		if let Some(Control::Dropdown(picks) | Control::Radio(picks)) =
			self.items.get_mut(index).map(|item| &mut item.control)
		{
			picks.picked.iter_mut().for_each(|picked| *picked = false);
			picks.picked[choice] = true;
		}
		cx.notify();
	}

	fn toggle(&mut self, index: usize, choice: usize, value: bool, cx: &mut Context<Self>) {
		match self.items.get_mut(index).map(|item| &mut item.control) {
			Some(Control::Toggles(picks)) => picks.picked[choice] = value,
			Some(Control::Flag(flag)) => *flag = value,
			_ => {}
		}
		cx.notify();
	}

	fn control(&self, index: usize, item: &Item, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		match &item.control {
			Control::Text {
				input: TextBox::Short(input),
				..
			} => Input::new(input).into_any_element(),
			Control::Text {
				input: TextBox::Paragraph(input),
				..
			} => Textarea::new(input).into_any_element(),
			Control::Dropdown(picks) => {
				let labels = Self::labels(picks);
				let selected = picks.picked.iter().position(|picked| *picked);
				let current = selected.map_or_else(
					|| cx.t("component-select-placeholder"),
					|selected| labels[selected].clone(),
				);
				select(
					("modal-select", index),
					current,
					&labels,
					selected.unwrap_or(usize::MAX),
					self.pick_handler(index, cx),
				)
				.into_any_element()
			}
			Control::Radio(picks) => {
				let labels = Self::labels(picks);
				let selected = picks
					.picked
					.iter()
					.position(|picked| *picked)
					.unwrap_or(usize::MAX);
				segmented(
					"modal-radio",
					&labels,
					selected,
					theme,
					self.pick_handler(index, cx),
				)
			}
			Control::Toggles(picks) => div()
				.flex()
				.flex_col()
				.gap(theme.space_8)
				.children(picks.options.iter().zip(&picks.picked).enumerate().map(
					|(choice, (option, picked))| {
						switch_row(
							(SharedString::from(format!("modal-toggle-{index}")), choice),
							option.label.clone(),
							option.description.clone().map(Into::into),
							*picked,
							theme,
							self.toggle_handler(index, choice, cx),
						)
					},
				))
				.into_any_element(),
			Control::Flag(flag) => switch_row(
				("modal-flag", index),
				item.label.clone(),
				item.description.clone(),
				*flag,
				theme,
				self.toggle_handler(index, 0, cx),
			),
		}
	}

	fn labels(picks: &Choices) -> Vec<SharedString> {
		picks
			.options
			.iter()
			.map(|option| option.label.clone().into())
			.collect()
	}

	fn pick_handler(&self, index: usize, cx: &Context<Self>) -> PickHandler {
		let weak = cx.weak_entity();
		Rc::new(move |choice, _, cx| {
			weak.update(cx, |this, cx| this.pick(index, choice, cx)).ok();
		})
	}

	fn toggle_handler(&self, index: usize, choice: usize, cx: &Context<Self>) -> ToggleHandler {
		let weak = cx.weak_entity();
		Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| this.toggle(index, choice, value, cx))
				.ok();
		})
	}
}

impl Render for ModalForm {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let notes: Vec<AnyElement> = self.notes.iter().map(|note| hint(note.clone(), &theme)).collect();
		let fields: Vec<AnyElement> = self
			.items
			.iter()
			.enumerate()
			.map(|(index, item)| {
				let control = self.control(index, item, &theme, cx);
				if matches!(item.control, Control::Flag(_)) {
					return control;
				}
				let body = div()
					.flex()
					.flex_col()
					.gap(theme.space_4)
					.children(item.description.clone().map(|text| hint(text, &theme)))
					.child(control);
				field(item.label.clone(), &theme, body)
			})
			.collect();
		let weak = cx.weak_entity();
		let cancel = weak.clone();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.children(notes)
			.children(fields)
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("interaction-modal-submit"),
				false,
				self.ready(cx),
				&theme,
				move |_, cx| {
					cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak.update(cx, |this, cx| this.submit(cx)).ok();
				},
			))
	}
}
