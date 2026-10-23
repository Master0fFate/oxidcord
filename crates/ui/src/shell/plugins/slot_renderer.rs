use std::rc::Rc;
use std::sync::Arc;

use gpui::{
	AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FontWeight, InteractiveElement as _,
	IntoElement, ObjectFit, ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _,
	Styled as _, StyledImage as _, Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::progress::Progress;
use gpui_component::switch::Switch;
use media::SizeHint;
use plugin_host::SlotEntry;
use plugin_host::api::{UiBox, UiButton, UiNode, UiText, UiTree};

use super::bridge::{PluginBridge, act, flush_inputs, forget_input, note_input, remember_anchor};
use super::icon::{self, PluginIcon};
use crate::components::{Divider, tooltip};
use crate::images::Images;
use crate::shell::links;
use crate::theme::{Theme, parse_color, rem};

const MAX_TEXT_SIZE: u8 = 48;
const MIN_TEXT_SIZE: u8 = 8;
const MAX_BOX_SPACING: u8 = 64;
const IMAGE_DEFAULT: u32 = 64;
const IMAGE_MAX: u32 = 512;
const BADGE_HEIGHT: f32 = 18.;
const INPUT_HEIGHT: f32 = 32.;
const INPUT_MIN_WIDTH: f32 = 220.;
const PROGRESS_WIDTH: f32 = 160.;
const LIGHT_FILL: f32 = 0.65;

#[derive(Clone, Copy)]
pub struct Compact {
	pub icon: f32,
	pub hit: f32,
	pub label: bool,
}

pub const MESSAGE_ACTION: Compact = Compact {
	icon: 18.,
	hit: 28.,
	label: false,
};
pub const COMPOSER: Compact = Compact {
	icon: 24.,
	hit: 32.,
	label: false,
};
pub const TITLE: Compact = Compact {
	icon: 14.,
	hit: 22.,
	label: true,
};
pub const STATUS: Compact = Compact {
	icon: 14.,
	hit: 22.,
	label: true,
};

#[derive(Clone)]
pub struct Scope {
	pub slot: &'static str,
	pub plugin: Arc<str>,
	pub payload: SharedString,
	pub compact: Option<Compact>,
}

pub struct SlotHost {
	slot: &'static str,
	payload: SharedString,
	compact: Option<Compact>,
}

impl SlotHost {
	pub fn new(slot: &'static str) -> Self {
		Self {
			slot,
			payload: SharedString::default(),
			compact: None,
		}
	}

	pub fn payload(mut self, payload: impl Into<SharedString>) -> Self {
		self.payload = payload.into();
		self
	}

	pub fn compact(mut self, compact: Compact) -> Self {
		self.compact = Some(compact);
		self
	}

	pub fn entries(&self, cx: &App) -> Rc<Vec<SlotEntry>> {
		cx.global::<PluginBridge>().entries(self.slot, cx)
	}

	pub fn scope(&self, entry: &SlotEntry) -> Scope {
		Scope {
			slot: self.slot,
			plugin: Arc::clone(&entry.plugin_id),
			payload: self.payload.clone(),
			compact: self.compact,
		}
	}

	pub fn render(&self, cx: &App) -> Vec<(Arc<str>, AnyElement)> {
		let theme = cx.global::<Theme>();
		self.entries(cx)
			.iter()
			.map(|entry| {
				let scope = self.scope(entry);
				let body = if self.compact.is_some() {
					render_row(&entry.tree, &scope, theme)
				} else {
					render_column(&entry.tree, &scope, theme)
				};
				(Arc::clone(&entry.plugin_id), body)
			})
			.collect()
	}
}

pub fn render_column(tree: &UiTree, scope: &Scope, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.items_start()
		.gap(theme.space_4)
		.children(
			tree.roots
				.iter()
				.map(|root| render_node(tree, *root as usize, scope, theme)),
		)
		.into_any_element()
}

pub fn render_row(tree: &UiTree, scope: &Scope, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.flex_none()
		.items_center()
		.gap(theme.space_4)
		.children(
			tree.roots
				.iter()
				.map(|root| render_node(tree, *root as usize, scope, theme)),
		)
		.into_any_element()
}

pub fn render_node(tree: &UiTree, index: usize, scope: &Scope, theme: &Theme) -> AnyElement {
	let Some(node) = tree.nodes.get(index) else {
		return div().into_any_element();
	};
	match node {
		UiNode::Column(layout) => container(tree, layout, false, scope, theme),
		UiNode::Row(layout) => container(tree, layout, true, scope, theme),
		UiNode::Text(text) => text_node(text, theme),
		UiNode::Button(button) => button_node(button, scope, theme),
		UiNode::Input(input) => PluginInput {
			scope: scope.clone(),
			id: input.id.clone().into(),
			placeholder: input.placeholder.clone().into(),
			value: input.value.clone().into(),
		}
		.into_any_element(),
		UiNode::Toggle(toggle) => {
			let plugin = Arc::clone(&scope.plugin);
			let id: SharedString = toggle.id.clone().into();
			let element = element_id("toggle", scope, &toggle.id);
			Switch::new(element)
				.checked(toggle.on)
				.label(toggle.label.clone())
				.on_click(move |checked: &bool, _, cx| {
					act(&plugin, &id, if *checked { "true" } else { "false" }, cx)
				})
				.into_any_element()
		}
		UiNode::Image(image) => PluginImage {
			url: image.url.clone().into(),
			width: image.width,
			height: image.height,
		}
		.into_any_element(),
		UiNode::Badge(badge) => badge_node(&badge.label, badge.color.as_deref(), theme),
		UiNode::Divider => div()
			.w_full()
			.py(theme.space_4)
			.child(Divider::horizontal())
			.into_any_element(),
		UiNode::Icon(name) => PluginIcon::new(name.clone()).size(rem(20.)).into_any_element(),
		UiNode::Progress(fraction) => {
			let element = element_id("progress", scope, "bar");
			div()
				.w(rem(PROGRESS_WIDTH))
				.child(Progress::new(element).value(fraction.clamp(0., 1.) * 100.))
				.into_any_element()
		}
		UiNode::Link(link) => link_node(&link.label, &link.url, scope, theme),
	}
}

fn element_id(kind: &str, scope: &Scope, id: &str) -> SharedString {
	format!(
		"plugin-{kind}:{}:{}:{}:{id}",
		scope.slot, scope.plugin, scope.payload
	)
	.into()
}

fn spacing(value: u8) -> gpui::Rems {
	rem(f32::from(value.min(MAX_BOX_SPACING)))
}

fn aligned(base: Div, row: bool, align: &str) -> Div {
	match (row, align) {
		(_, "center") => base.items_center().when(row, |layout| layout.justify_center()),
		(_, "end") => base.items_end().when(row, |layout| layout.justify_end()),
		(true, "between") => base.items_center().justify_between(),
		(true, "start") => base.items_start(),
		(true, _) => base.items_center(),
		(false, "stretch") => base,
		(false, _) => base.items_start(),
	}
}

fn container(tree: &UiTree, layout: &UiBox, row: bool, scope: &Scope, theme: &Theme) -> AnyElement {
	let base = div()
		.flex()
		.when(!row, |layout| layout.flex_col())
		.gap(spacing(layout.gap))
		.p(spacing(layout.padding));
	aligned(base, row, &layout.align)
		.children(
			layout
				.children
				.iter()
				.map(|child| render_node(tree, *child as usize, scope, theme)),
		)
		.into_any_element()
}

fn text_node(text: &UiText, theme: &Theme) -> AnyElement {
	let style = &text.style;
	let size = if style.size == 0 {
		theme.font_sm
	} else {
		rem(f32::from(style.size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE)))
	};
	let color = style
		.color
		.as_deref()
		.and_then(parse_color)
		.unwrap_or(if style.muted {
			theme.text_muted
		} else {
			theme.text_normal
		});
	div()
		.text_size(size)
		.text_color(color)
		.when(style.bold, |label| label.font_weight(FontWeight::SEMIBOLD))
		.child(text.value.clone())
		.into_any_element()
}

fn badge_node(label: &str, color: Option<&str>, theme: &Theme) -> AnyElement {
	let fill = color.and_then(parse_color).unwrap_or(theme.brand);
	let text = if fill.l > LIGHT_FILL {
		theme.background_tertiary
	} else {
		theme.text_on_brand
	};
	div()
		.flex()
		.flex_none()
		.items_center()
		.h(rem(BADGE_HEIGHT))
		.px(theme.space_8)
		.rounded(theme.radius_sm)
		.bg(fill)
		.text_color(text)
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.child(label.to_owned())
		.into_any_element()
}

fn link_node(label: &str, url: &str, scope: &Scope, theme: &Theme) -> AnyElement {
	let url = url.to_owned();
	div()
		.id(element_id("link", scope, &url))
		.cursor_pointer()
		.text_size(theme.font_sm)
		.text_color(theme.link)
		.hover(|style| style.underline())
		.child(label.to_owned())
		.on_click(move |_, window, cx| {
			let backend = cx.global::<PluginBridge>().backend().clone();
			links::open_link(&url, &backend, window, cx);
		})
		.into_any_element()
}

fn button_node(button: &UiButton, scope: &Scope, theme: &Theme) -> AnyElement {
	let plugin = Arc::clone(&scope.plugin);
	let id: SharedString = button.id.clone().into();
	let payload = scope.payload.clone();
	let element = element_id("button", scope, &button.id);
	let slot = scope.slot;
	let icon = button.icon.as_deref().and_then(icon::resolve);
	if let Some(compact) = scope.compact {
		return compact_button(
			element,
			&button.label,
			button.icon.as_deref(),
			compact,
			theme,
			move |event, _, cx| {
				if slot == "composer-toolbar" {
					remember_anchor(event.position(), cx);
				}
				act(&plugin, &id, &payload, cx)
			},
		);
	}
	let mut widget = Button::new(element)
		.small()
		.label(button.label.clone())
		.on_click(move |_, _, cx| {
			flush_inputs(&plugin, cx);
			act(&plugin, &id, &payload, cx);
		});
	if let Some(path) = icon {
		widget = widget.icon(gpui_component::Icon::default().path(path));
	}
	let widget = match button.variant.as_str() {
		"primary" => widget.primary(),
		"danger" => widget.danger(),
		"ghost" => widget.ghost(),
		"outline" => widget.outline(),
		_ => widget,
	};
	widget.into_any_element()
}

fn compact_button(
	element: SharedString,
	label: &str,
	icon_name: Option<&str>,
	compact: Compact,
	theme: &Theme,
	on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
	let has_icon = icon_name.is_some_and(|name| icon::resolve(name).is_some());
	let show_label = compact.label || !has_icon;
	let hover = theme.modifier_hover;
	let hover_text = theme.interactive_hover;
	let tip = (!show_label).then(|| label.to_owned());
	div()
		.id(element)
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.gap(theme.space_4)
		.h(rem(compact.hit))
		.min_w(rem(compact.hit))
		.px(theme.space_4)
		.rounded(theme.radius_sm)
		.cursor_pointer()
		.text_size(theme.font_xs)
		.font_weight(FontWeight::MEDIUM)
		.text_color(theme.interactive_normal)
		.hover(move |style| style.bg(hover).text_color(hover_text))
		.when_some(tip, |button, tip| button.tooltip(tooltip(tip)))
		.on_click(on_click)
		.children(icon_name.filter(|_| has_icon).map(|name| {
			PluginIcon::new(name.to_owned())
				.size(rem(compact.icon))
				.color(theme.interactive_normal)
		}))
		.when(show_label, |button| button.child(label.to_owned()))
		.into_any_element()
}

#[derive(IntoElement)]
struct PluginImage {
	url: SharedString,
	width: u32,
	height: u32,
}

impl RenderOnce for PluginImage {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>();
		let (width, height) = match (self.width, self.height) {
			(0, 0) => (IMAGE_DEFAULT, IMAGE_DEFAULT),
			(0, height) => (height, height),
			(width, 0) => (width, width),
			pair => pair,
		};
		let (width, height) = (width.min(IMAGE_MAX), height.min(IMAGE_MAX));
		let image = cx.global::<Images>().get(&self.url, SizeHint::new(width, height));
		div()
			.flex_none()
			.w(rem(width as f32))
			.h(rem(height as f32))
			.rounded(theme.radius_sm)
			.overflow_hidden()
			.bg(theme.background_secondary)
			.children(image.map(|image| img(image).size_full().object_fit(ObjectFit::Cover)))
	}
}

struct InputBinding {
	input: Entity<InputState>,
	declared: SharedString,
	placeholder: SharedString,
	_subscription: Subscription,
}

impl InputBinding {
	fn new(
		plugin: Arc<str>,
		id: SharedString,
		value: &SharedString,
		placeholder: &SharedString,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let input = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder.clone());
			state.set_value(value.clone(), window, cx);
			state
		});
		let subscription = cx.subscribe_in(
			&input,
			window,
			move |this, input, event: &InputEvent, window, cx| {
				let text = match event {
					InputEvent::Change => {
						note_input(&plugin, &id, input.read(cx).value(), cx);
						return;
					}
					InputEvent::PressEnter { .. } => {
						let text = input.read(cx).value();
						if this.declared.is_empty() {
							input.update(cx, |state, cx| state.set_value("", window, cx));
						}
						text
					}
					InputEvent::Blur => input.read(cx).value(),
					_ => return,
				};
				forget_input(&plugin, &id, cx);
				act(&plugin, &id, &text, cx);
			},
		);
		Self {
			input,
			declared: value.clone(),
			placeholder: placeholder.clone(),
			_subscription: subscription,
		}
	}

	fn sync(
		&mut self,
		value: &SharedString,
		placeholder: &SharedString,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.declared != *value {
			self.declared = value.clone();
			self.input
				.update(cx, |state, cx| state.set_value(value.clone(), window, cx));
		}
		if self.placeholder != *placeholder {
			self.placeholder = placeholder.clone();
			self.input.update(cx, |state, cx| {
				state.set_placeholder(placeholder.clone(), window, cx)
			});
		}
	}
}

#[derive(IntoElement)]
struct PluginInput {
	scope: Scope,
	id: SharedString,
	placeholder: SharedString,
	value: SharedString,
}

impl RenderOnce for PluginInput {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let key = element_id("input", &self.scope, &self.id);
		let plugin = Arc::clone(&self.scope.plugin);
		let (id, value, placeholder) = (self.id.clone(), self.value.clone(), self.placeholder.clone());
		let binding = window.use_keyed_state(key, cx, move |window, cx| {
			InputBinding::new(plugin, id, &value, &placeholder, window, cx)
		});
		binding.update(cx, |binding, cx| {
			binding.sync(&self.value, &self.placeholder, window, cx)
		});
		let input = binding.read(cx).input.clone();
		div()
			.flex()
			.items_center()
			.flex_1()
			.min_w(rem(INPUT_MIN_WIDTH))
			.h(rem(INPUT_HEIGHT))
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.bg(theme.input_background)
			.child(
				Input::new(&input)
					.appearance(false)
					.bordered(false)
					.focus_bordered(false)
					.small()
					.w_full(),
			)
	}
}
