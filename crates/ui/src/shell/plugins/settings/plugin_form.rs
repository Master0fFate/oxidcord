use gpui::{
	AnyElement, AppContext as _, ClickEvent, Context, Entity, FontWeight, InteractiveElement as _,
	IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	Subscription, Window, div,
};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::switch::Switch;
use serde_json::Value;
use session::Command;

use super::widgets::note;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::plugins::bridge::{PluginBridge, stored_setting};
use crate::theme::{Theme, rem};

const INPUT_WIDTH: f32 = 320.;
const INPUT_HEIGHT: f32 = 32.;
const TAG_HEIGHT: f32 = 24.;

enum Kind {
	Text,
	Number {
		min: Option<f64>,
		max: Option<f64>,
		integer: bool,
	},
	Toggle,
	Choice(Vec<String>),
	Tags,
}

struct Field {
	key: String,
	title: String,
	description: Option<String>,
	kind: Kind,
	value: String,
	tags: Vec<String>,
	input: Option<Entity<InputState>>,
}

pub struct PluginForm {
	plugin_id: String,
	name: SharedString,
	fields: Vec<Field>,
	_subscriptions: Vec<Subscription>,
}

fn text_of(value: &Value) -> String {
	match value {
		Value::String(text) => text.clone(),
		other => other.to_string(),
	}
}

fn kind_of(spec: &Value) -> Kind {
	if let Some(options) = spec.get("enum").and_then(Value::as_array) {
		return Kind::Choice(options.iter().map(text_of).collect());
	}
	match spec.get("type").and_then(Value::as_str) {
		Some("boolean") => Kind::Toggle,
		Some(kind @ ("integer" | "number")) => Kind::Number {
			min: spec.get("minimum").and_then(Value::as_f64),
			max: spec.get("maximum").and_then(Value::as_f64),
			integer: kind == "integer",
		},
		Some("array") => Kind::Tags,
		_ => Kind::Text,
	}
}

fn clamp_number(value: f64, min: Option<f64>, max: Option<f64>, integer: bool) -> f64 {
	let value = min.map_or(value, |min| value.max(min));
	let value = max.map_or(value, |max| value.min(max));
	if integer { value.round() } else { value }
}

fn format_number(value: f64, integer: bool) -> String {
	if integer || value.fract() == 0. {
		format!("{value:.0}")
	} else {
		value.to_string()
	}
}

impl PluginForm {
	pub fn new(
		plugin_id: String,
		name: SharedString,
		schema: Option<Value>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let mut form = Self {
			plugin_id,
			name,
			fields: Vec::new(),
			_subscriptions: Vec::new(),
		};
		let properties = schema
			.as_ref()
			.and_then(|schema| schema.get("properties"))
			.and_then(Value::as_object);
		for (key, spec) in properties.into_iter().flatten() {
			form.add_field(key, spec, window, cx);
		}
		form
	}

	fn add_field(&mut self, key: &str, spec: &Value, window: &mut Window, cx: &mut Context<Self>) {
		let kind = kind_of(spec);
		let value = stored_setting(&self.plugin_id, key, cx)
			.or_else(|| spec.get("default").map(text_of))
			.unwrap_or_default();
		let tags = if matches!(kind, Kind::Tags) {
			serde_json::from_str::<Vec<String>>(&value).unwrap_or_default()
		} else {
			Vec::new()
		};
		let input = match kind {
			Kind::Text | Kind::Number { .. } | Kind::Tags => {
				let initial = if matches!(kind, Kind::Tags) {
					String::new()
				} else {
					value.clone()
				};
				let placeholder = cx.t("plugin-form-add-tag");
				let tags_field = matches!(kind, Kind::Tags);
				let input = cx.new(|cx| {
					let mut state = InputState::new(window, cx);
					if tags_field {
						state = state.placeholder(placeholder);
					}
					state.set_value(initial, window, cx);
					state
				});
				let index = self.fields.len();
				let subscription = cx.subscribe_in(
					&input,
					window,
					move |this, _, event: &InputEvent, window, cx| match event {
						InputEvent::PressEnter { .. } => this.commit(index, true, window, cx),
						InputEvent::Blur => this.commit(index, false, window, cx),
						_ => {}
					},
				);
				self._subscriptions.push(subscription);
				Some(input)
			}
			Kind::Toggle | Kind::Choice(_) => None,
		};
		self.fields.push(Field {
			key: key.to_owned(),
			title: spec
				.get("title")
				.and_then(Value::as_str)
				.unwrap_or(key)
				.to_owned(),
			description: spec.get("description").and_then(Value::as_str).map(str::to_owned),
			kind,
			value,
			tags,
			input,
		});
	}

	fn send(&self, index: usize, cx: &mut Context<Self>) {
		let Some(field) = self.fields.get(index) else {
			return;
		};
		let value = if matches!(field.kind, Kind::Tags) {
			serde_json::to_string(&field.tags).unwrap_or_default()
		} else {
			field.value.clone()
		};
		cx.global::<PluginBridge>().send(
			Command::PluginSetSetting {
				id: self.plugin_id.clone(),
				key: field.key.clone(),
				value,
			},
			cx,
		);
	}

	fn commit(&mut self, index: usize, entered: bool, window: &mut Window, cx: &mut Context<Self>) {
		let Some(field) = self.fields.get_mut(index) else {
			return;
		};
		let Some(input) = field.input.clone() else {
			return;
		};
		let text = input.read(cx).value().trim().to_owned();
		match &field.kind {
			Kind::Text => {
				if field.value == text {
					return;
				}
				field.value = text;
			}
			Kind::Number { min, max, integer } => {
				let Ok(parsed) = text.parse::<f64>() else {
					let previous = field.value.clone();
					input.update(cx, |state, cx| state.set_value(previous, window, cx));
					return;
				};
				let canonical = format_number(clamp_number(parsed, *min, *max, *integer), *integer);
				input.update(cx, |state, cx| state.set_value(canonical.clone(), window, cx));
				if field.value == canonical {
					return;
				}
				field.value = canonical;
			}
			Kind::Tags => {
				if !entered || text.is_empty() {
					return;
				}
				if !field.tags.contains(&text) {
					field.tags.push(text);
				}
				input.update(cx, |state, cx| state.set_value("", window, cx));
			}
			Kind::Toggle | Kind::Choice(_) => return,
		}
		self.send(index, cx);
		cx.notify();
	}

	fn set_value(&mut self, index: usize, value: String, cx: &mut Context<Self>) {
		if let Some(field) = self.fields.get_mut(index) {
			field.value = value;
		}
		self.send(index, cx);
		cx.notify();
	}

	fn remove_tag(&mut self, index: usize, tag: usize, cx: &mut Context<Self>) {
		if let Some(field) = self.fields.get_mut(index)
			&& tag < field.tags.len()
		{
			field.tags.remove(tag);
			self.send(index, cx);
			cx.notify();
		}
	}

	fn text_input(input: &Entity<InputState>, theme: &Theme) -> AnyElement {
		div()
			.flex()
			.items_center()
			.w(rem(INPUT_WIDTH))
			.h(rem(INPUT_HEIGHT))
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.bg(theme.input_background)
			.child(
				Input::new(input)
					.appearance(false)
					.bordered(false)
					.focus_bordered(false)
					.small()
					.w_full(),
			)
			.into_any_element()
	}

	fn control(&self, index: usize, field: &Field, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		match &field.kind {
			Kind::Toggle => Switch::new(("plugin-field-toggle", index))
				.checked(field.value == "true")
				.on_change(cx.listener(move |this, checked: &bool, _, cx| {
					this.set_value(index, checked.to_string(), cx)
				}))
				.into_any_element(),
			Kind::Choice(options) => {
				let weak = cx.weak_entity();
				let options = options.clone();
				let current = field.value.clone();
				Button::new(("plugin-field-choice", index))
					.outline()
					.small()
					.label(field.value.clone())
					.dropdown_menu(move |menu, _, _| {
						options.iter().fold(menu, |menu, option| {
							let weak = weak.clone();
							let chosen = option.clone();
							menu.item(
								PopupMenuItem::new(option.clone())
									.checked(*option == current)
									.on_click(move |_: &ClickEvent, _, cx| {
										let chosen = chosen.clone();
										weak.update(cx, |this, cx| this.set_value(index, chosen, cx)).ok();
									}),
							)
						})
					})
					.into_any_element()
			}
			Kind::Number { min, max, .. } => {
				let range = match (min, max) {
					(Some(min), Some(max)) => Some(cx.t_args(
						"plugin-form-range",
						&[
							("min", ArgValue::from(min.to_string())),
							("max", ArgValue::from(max.to_string())),
						],
					)),
					_ => None,
				};
				div()
					.flex()
					.items_center()
					.gap(theme.space_12)
					.children(field.input.as_ref().map(|input| Self::text_input(input, theme)))
					.children(range.map(|range| note(range, theme)))
					.into_any_element()
			}
			Kind::Text => div()
				.children(field.input.as_ref().map(|input| Self::text_input(input, theme)))
				.into_any_element(),
			Kind::Tags => {
				let chips: Vec<AnyElement> = field
					.tags
					.iter()
					.enumerate()
					.map(|(tag_index, tag)| {
						div()
							.flex()
							.items_center()
							.gap(theme.space_4)
							.h(rem(TAG_HEIGHT))
							.pl(theme.space_8)
							.pr(theme.space_4)
							.rounded(theme.radius_sm)
							.bg(theme.background_secondary)
							.text_size(theme.font_sm)
							.text_color(theme.text_normal)
							.child(tag.clone())
							.child(
								div()
									.id(("plugin-tag-remove", index * 1000 + tag_index))
									.flex()
									.items_center()
									.cursor_pointer()
									.on_click(cx.listener(move |this, _, _, cx| {
										this.remove_tag(index, tag_index, cx)
									}))
									.child(
										Icon::new(IconKind::Close)
											.size(rem(14.))
											.color(theme.interactive_normal),
									),
							)
							.into_any_element()
					})
					.collect();
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(div().flex().flex_wrap().gap(theme.space_8).children(chips))
					.children(field.input.as_ref().map(|input| Self::text_input(input, theme)))
					.into_any_element()
			}
		}
	}
}

impl Render for PluginForm {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		if self.fields.is_empty() {
			return note(cx.t("plugin-form-empty"), &theme);
		}
		let fields: Vec<AnyElement> = self
			.fields
			.iter()
			.enumerate()
			.map(|(index, field)| {
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(
						div()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(field.title.clone()),
					)
					.children(
						field
							.description
							.clone()
							.map(|description| note(description, &theme)),
					)
					.child(self.control(index, field, &theme, cx))
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.children(fields)
			.into_any_element()
	}
}

impl PluginForm {
	pub fn name(&self) -> &SharedString {
		&self.name
	}
}
