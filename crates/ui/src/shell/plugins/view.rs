use std::rc::Rc;
use std::sync::Arc;

use gpui::{
	AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FontWeight, Hsla,
	InteractiveElement as _, IntoElement, MouseButton, ObjectFit, ParentElement as _, RenderOnce,
	SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Window,
	anchored, deferred, div, img, prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::progress::Progress;
use gpui_component::switch::Switch;
use gpui_component::{Disableable as _, Sizable as _};
use markdown::TimestampStyle;
use media::SizeHint;
use plugin_host::ViewContext;
use plugin_host::api::model::{
	Align, Axis, ButtonNode, ButtonVariant, Color, EmbedCard, EmojiRef, ImageSource, Justify, Length,
	NodeKind, Radius, Space, Style, TextNode, TextSize, Tone, UiContext, ViewEvent, ViewEventKind, ViewTree,
	Weight,
};
use protocol::Snowflake;
use session::Command;

use super::bridge::{PluginBridge, act, detail_of, flush_inputs, forget_input, note_input, remember_anchor};
use super::icon::{self, PluginIcon};
use crate::components::{Avatar, Divider, SectionHeader, tooltip};
use crate::format;
use crate::images::{Images, frame_id};
use crate::lookup::avatar_for;
use crate::shell::account::rich::markdown_text;
use crate::shell::links;
use crate::theme::{Theme, rem};

const IMAGE_DEFAULT: u32 = 64;
const IMAGE_MAX: u32 = 512;
const ICON_DEFAULT: u32 = 20;
const BADGE_HEIGHT: f32 = 18.;
const INPUT_HEIGHT: f32 = 32.;
const INPUT_MIN_WIDTH: f32 = 220.;
const PROGRESS_WIDTH: f32 = 160.;
const LIGHT_FILL: f32 = 0.65;
const DISABLED_OPACITY: f32 = 0.5;
const EMBED_MAX_WIDTH: f32 = 432.;
const EMBED_IMAGE: u32 = 400;
const CHIP_AVATAR: f32 = 18.;
const POPOVER_MIN_WIDTH: f32 = 220.;
const POPOVER_MAX_WIDTH: f32 = 360.;

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
pub const PANEL: Compact = Compact {
	icon: 16.,
	hit: 28.,
	label: false,
};
pub const INLINE: Compact = Compact {
	icon: 14.,
	hit: 20.,
	label: false,
};

#[derive(Clone)]
pub enum Origin {
	Slot {
		slot: &'static str,
		payload: SharedString,
	},
	Mount {
		point: SharedString,
		mount: SharedString,
		context: ViewContext,
	},
}

#[derive(Clone)]
struct FormScope {
	id: SharedString,
	declared: Rc<Vec<(String, String)>>,
}

#[derive(Clone)]
pub struct Scope {
	pub plugin: Arc<str>,
	pub origin: Origin,
	pub compact: Option<Compact>,
	form: Option<FormScope>,
}

impl Scope {
	pub fn slot(plugin: &Arc<str>, slot: &'static str, payload: impl Into<SharedString>) -> Self {
		Self {
			plugin: Arc::clone(plugin),
			origin: Origin::Slot {
				slot,
				payload: payload.into(),
			},
			compact: None,
			form: None,
		}
	}

	pub fn mount(
		plugin: &Arc<str>,
		point: impl Into<SharedString>,
		mount: impl Into<SharedString>,
		context: ViewContext,
	) -> Self {
		Self {
			plugin: Arc::clone(plugin),
			origin: Origin::Mount {
				point: point.into(),
				mount: mount.into(),
				context,
			},
			compact: None,
			form: None,
		}
	}

	pub fn compact(mut self, compact: Option<Compact>) -> Self {
		self.compact = compact;
		self
	}

	fn origin_id(&self) -> String {
		match &self.origin {
			Origin::Slot { slot, payload } => format!("{slot}:{payload}"),
			Origin::Mount {
				point,
				mount,
				context,
			} => format!(
				"{point}:{mount}:{}:{}:{}:{}:{}",
				context.guild_id.unwrap_or(0),
				context.channel_id.unwrap_or(0),
				context.message_id.unwrap_or(0),
				context.user_id.unwrap_or(0),
				context.attachment_id.unwrap_or(0)
			),
		}
	}

	fn element_id(&self, kind: &str, key: &str) -> SharedString {
		format!("plugin-{kind}:{}:{}:{key}", self.plugin, self.origin_id()).into()
	}

	fn field_id(&self, kind: &str, key: &str, cx: &App) -> SharedString {
		match &self.form {
			Some(form) => {
				let generation = cx.global::<PluginBridge>().form_generation(&form.id);
				format!("plugin-{kind}:{}:{generation}:{key}", form.id).into()
			}
			None => self.element_id(kind, key),
		}
	}

	fn legacy(&self) -> bool {
		matches!(self.origin, Origin::Slot { .. })
	}
}

pub enum Fire {
	Click,
	Change(String),
	Toggle(bool),
	Select(String),
	Tab(u32),
	Submit(Vec<(String, String)>),
}

fn context_of(context: ViewContext) -> UiContext {
	context.to_api(None)
}

pub fn fire(scope: &Scope, key: &str, event: Fire, cx: &mut App) {
	match &scope.origin {
		Origin::Slot { slot, payload } => {
			let payload = match event {
				Fire::Click => {
					if *slot != "composer-toolbar" {
						flush_inputs(&scope.plugin, cx);
					}
					payload.to_string()
				}
				Fire::Change(text) => text,
				Fire::Toggle(on) => if on { "true" } else { "false" }.to_owned(),
				Fire::Select(_) | Fire::Tab(_) | Fire::Submit(_) => return,
			};
			act(&scope.plugin, key, &payload, cx);
		}
		Origin::Mount {
			point,
			mount,
			context,
		} => {
			let kind = match event {
				Fire::Click => ViewEventKind::Click,
				Fire::Change(text) => ViewEventKind::Change(text),
				Fire::Toggle(on) => ViewEventKind::Toggle(on),
				Fire::Select(value) => ViewEventKind::Select(value),
				Fire::Tab(index) => ViewEventKind::Tab(index),
				Fire::Submit(values) => ViewEventKind::Submit(values),
			};
			cx.global::<PluginBridge>().send(
				Command::PluginViewEvent {
					plugin_id: scope.plugin.to_string(),
					event: ViewEvent {
						point: point.to_string(),
						mount: mount.to_string(),
						node: key.to_owned(),
						kind,
						context: context_of(*context),
					},
					detail: detail_of(*context),
				},
				cx,
			);
		}
	}
}

fn submit(scope: &Scope, key: &str, cx: &mut App) {
	let Some(form) = &scope.form else {
		fire(scope, key, Fire::Click, cx);
		return;
	};
	let mut changed = cx.global::<PluginBridge>().take_form(&form.id);
	let mut values: Vec<(String, String)> = form
		.declared
		.iter()
		.map(|(field, declared)| {
			let value = changed.remove(field).unwrap_or_else(|| declared.clone());
			(field.clone(), value)
		})
		.collect();
	values.extend(changed);
	let form_key = form.id.rsplit(':').next().unwrap_or_default().to_owned();
	fire(scope, &form_key, Fire::Submit(values), cx);
}

fn change(scope: &Scope, key: &str, value: String, fire_event: Fire, cx: &mut App) {
	match &scope.form {
		Some(form) => cx.global::<PluginBridge>().record_field(&form.id, key, value),
		None => fire(scope, key, fire_event, cx),
	}
}

pub fn resolve(color: &Color, theme: &Theme) -> Option<Hsla> {
	match color {
		Color::Token(token) => theme.token(token),
		Color::Rgba(rgba) => Some(gpui::rgba(*rgba).into()),
	}
}

fn tone_color(tone: Tone, theme: &Theme) -> Hsla {
	match tone {
		Tone::Normal => theme.text_normal,
		Tone::Muted => theme.text_muted,
		Tone::Brand => theme.brand,
		Tone::Success => theme.success,
		Tone::Warning => theme.warning,
		Tone::Danger => theme.danger,
	}
}

fn space(value: Space, theme: &Theme) -> gpui::Rems {
	match value {
		Space::None => rem(0.),
		Space::Xs => theme.space_2,
		Space::Sm => theme.space_4,
		Space::Md => theme.space_8,
		Space::Lg => theme.space_16,
		Space::Xl => theme.space_24,
	}
}

fn text_size(size: TextSize, theme: &Theme) -> gpui::Rems {
	match size {
		TextSize::Tiny => theme.font_tiny,
		TextSize::Xs => theme.font_xs,
		TextSize::Sm => theme.font_sm,
		TextSize::Md => theme.font_md,
		TextSize::Lg => theme.font_lg,
		TextSize::Xl => theme.font_xl,
	}
}

fn weight(weight: Weight) -> FontWeight {
	match weight {
		Weight::Normal => FontWeight::NORMAL,
		Weight::Medium => FontWeight::MEDIUM,
		Weight::Semibold => FontWeight::SEMIBOLD,
		Weight::Bold => FontWeight::BOLD,
	}
}

fn sized(base: Div, style: &Style, row: bool) -> Div {
	let base = match (style.width, row) {
		(Length::Fill, true) => base.flex_1().min_w_0(),
		(Length::Fill, false) => base.w_full(),
		(Length::Px(px), _) => base.w(rem(px as f32)),
		(Length::Auto, _) => base,
	};
	match (style.height, row) {
		(Length::Fill, true) => base.h_full(),
		(Length::Fill, false) => base.flex_1().min_h_0(),
		(Length::Px(px), _) => base.h(rem(px as f32)),
		(Length::Auto, _) => base,
	}
}

fn styled(base: Div, style: Option<&Style>, row: bool, theme: &Theme) -> Div {
	let Some(style) = style else {
		return base;
	};
	let base = sized(base, style, row).p(space(style.padding, theme));
	let base = match style.radius {
		Radius::None => base,
		Radius::Sm => base.rounded(theme.radius_sm),
		Radius::Md => base.rounded(theme.radius_md),
		Radius::Lg => base.rounded(theme.radius_lg),
		Radius::Full => base.rounded_full(),
	};
	base.when_some(
		style.background.as_ref().and_then(|color| resolve(color, theme)),
		|base, fill| base.bg(fill),
	)
	.when_some(
		style.border.as_ref().and_then(|color| resolve(color, theme)),
		|base, line| base.border_1().border_color(line),
	)
	.when(style.disabled, |base| base.opacity(DISABLED_OPACITY))
}

fn laid_out(base: Div, style: Option<&Style>, axis: Axis, theme: &Theme) -> Div {
	let row = axis == Axis::Row;
	let base = base.flex().when(!row, |layout| layout.flex_col());
	let Some(style) = style else {
		return if row {
			base.items_center()
		} else {
			base.items_start()
		};
	};
	let base = base.gap(space(style.gap, theme));
	let base = match style.align {
		Align::Start => base.items_start(),
		Align::Center => base.items_center(),
		Align::End => base.items_end(),
		Align::Stretch => base,
	};
	match style.justify {
		Justify::Start => base,
		Justify::Center => base.justify_center(),
		Justify::End => base.justify_end(),
		Justify::Between => base.justify_between(),
	}
}

fn disabled(style: Option<&Style>) -> bool {
	style.is_some_and(|style| style.disabled)
}

fn tip(style: Option<&Style>) -> Option<SharedString> {
	style.and_then(|style| style.tooltip.clone()).map(Into::into)
}

#[derive(Clone)]
pub struct Ctx {
	pub tree: Arc<ViewTree>,
	pub scope: Scope,
}

impl Ctx {
	pub fn new(tree: &Arc<ViewTree>, scope: Scope) -> Self {
		Self {
			tree: Arc::clone(tree),
			scope,
		}
	}

	fn key_of(&self, index: usize) -> String {
		self.tree
			.nodes
			.get(index)
			.and_then(|node| node.key.clone())
			.unwrap_or_else(|| format!("n{index}"))
	}

	fn children(&self, indices: &[u32], row: bool, theme: &Theme) -> Vec<AnyElement> {
		indices
			.iter()
			.map(|child| self.node(*child as usize, row, theme))
			.collect()
	}

	fn within_form(&self, index: usize) -> Ctx {
		let mut declared = Vec::new();
		collect_fields(&self.tree, index, &mut declared, 0);
		let mut scope = self.scope.clone();
		scope.form = Some(FormScope {
			id: self.scope.element_id("form", &self.key_of(index)),
			declared: Rc::new(declared),
		});
		Ctx {
			tree: Arc::clone(&self.tree),
			scope,
		}
	}

	pub fn roots(&self, row: bool, theme: &Theme) -> Vec<AnyElement> {
		self.children(&self.tree.roots, row, theme)
	}

	pub fn column(&self, theme: &Theme) -> AnyElement {
		div()
			.flex()
			.flex_col()
			.items_start()
			.gap(theme.space_4)
			.min_w_0()
			.children(self.roots(false, theme))
			.into_any_element()
	}

	pub fn row(&self, theme: &Theme) -> AnyElement {
		self.row_of(usize::MAX, theme)
	}

	pub fn row_of(&self, limit: usize, theme: &Theme) -> AnyElement {
		let roots = self.tree.roots.iter().take(limit);
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_4)
			.children(roots.map(|root| self.node(*root as usize, true, theme)))
			.into_any_element()
	}

	pub fn node(&self, index: usize, row: bool, theme: &Theme) -> AnyElement {
		let Some(node) = self.tree.nodes.get(index) else {
			return div().into_any_element();
		};
		let style = node.style.as_ref();
		let key = self.key_of(index);
		let element = match &node.kind {
			NodeKind::Stack(stack) => {
				let inner_row = stack.axis == Axis::Row;
				styled(laid_out(div(), style, stack.axis, theme), style, row, theme)
					.when(stack.wrap, |layout| layout.flex_wrap())
					.children(self.children(&stack.children, inner_row, theme))
					.into_any_element()
			}
			NodeKind::Scroll(children) => styled(div(), style, row, theme)
				.id(self.scope.element_id("scroll", &key))
				.when(style.is_none_or(|style| style.height == Length::Auto), |list| {
					list.flex_1().min_h_0()
				})
				.overflow_y_scroll()
				.child(
					laid_out(div(), style, Axis::Column, theme)
						.w_full()
						.children(self.children(children, false, theme)),
				)
				.into_any_element(),
			NodeKind::Form(children) => {
				let form = self.within_form(index);
				styled(laid_out(div(), style, Axis::Column, theme), style, row, theme)
					.children(form.children(children, false, theme))
					.into_any_element()
			}
			NodeKind::Spacer => div().flex_1().into_any_element(),
			NodeKind::Divider => div()
				.w_full()
				.py(theme.space_4)
				.child(Divider::horizontal())
				.into_any_element(),
			NodeKind::Text(text) => styled(text_element(text, theme), style, row, theme).into_any_element(),
			NodeKind::Markdown(source) => styled(div().min_w_0(), style, row, theme)
				.child(markdown_text(source, theme))
				.into_any_element(),
			NodeKind::Image(image) => PluginImage {
				source: image.source.clone(),
				width: image.width,
				height: image.height,
			}
			.into_any_element(),
			NodeKind::Icon(node) => {
				let size = if node.size == 0 { ICON_DEFAULT } else { node.size };
				PluginIcon::new(node.name.clone())
					.size(rem(size as f32))
					.color(tone_color(node.tone, theme))
					.into_any_element()
			}
			NodeKind::Avatar(avatar) => UserBadge {
				user: Snowflake(avatar.user_id),
				size: avatar.size as f32,
				name: false,
			}
			.into_any_element(),
			NodeKind::Emoji(emoji) => emoji_element(&emoji.emoji, emoji.size, theme),
			NodeKind::Button(button) => self.button(button, &key, style, theme),
			NodeKind::IconButton(button) => {
				let scope = self.scope.clone();
				let key_owned = key.clone();
				icon_button(
					IconButtonSpec {
						element: self.scope.element_id("icon-button", &key),
						color: tone_color(button.tone, theme),
						tip: tip(style),
						inactive: disabled(style),
						size: self.scope.compact.map_or(16., |compact| compact.icon),
					},
					&button.icon,
					theme,
					move |_, _, cx| fire(&scope, &key_owned, Fire::Click, cx),
				)
			}
			NodeKind::Link(link) => {
				link_element(&link.label, &link.url, self.scope.element_id("link", &key), theme)
			}
			NodeKind::Input(input) | NodeKind::Textarea(input) => FieldInput {
				scope: self.scope.clone(),
				key: key.clone().into(),
				placeholder: input.placeholder.clone().into(),
				value: input.value.clone().into(),
				multiline: matches!(node.kind, NodeKind::Textarea(_)),
				disabled: disabled(style),
			}
			.into_any_element(),
			NodeKind::Select(select) => SelectField {
				scope: self.scope.clone(),
				key: key.clone().into(),
				options: select
					.options
					.iter()
					.map(|option| (option.value.clone().into(), option.label.clone().into()))
					.collect(),
				selected: select.selected.clone().map(Into::into),
				placeholder: select.placeholder.clone().into(),
			}
			.into_any_element(),
			NodeKind::Toggle(check) | NodeKind::Checkbox(check) => CheckField {
				scope: self.scope.clone(),
				key: key.clone().into(),
				label: check.label.clone().into(),
				checked: check.checked,
				switch: matches!(node.kind, NodeKind::Toggle(_)),
				disabled: disabled(style),
			}
			.into_any_element(),
			NodeKind::Slider(slider) => SliderField {
				scope: self.scope.clone(),
				key: key.clone().into(),
				min: slider.min,
				max: slider.max,
				step: slider.step,
				value: slider.value,
			}
			.into_any_element(),
			NodeKind::Progress(fraction) => div()
				.w(rem(PROGRESS_WIDTH))
				.child(Progress::new(self.scope.element_id("progress", &key)).value(fraction * 100.))
				.into_any_element(),
			NodeKind::Badge(badge) => {
				let fill = style
					.and_then(|style| style.background.as_ref())
					.and_then(|color| resolve(color, theme))
					.unwrap_or_else(|| tone_color(badge.tone, theme));
				badge_element(&badge.label, fill, theme)
			}
			NodeKind::Tabs(tabs) => TabsView {
				ctx: self.clone(),
				id: self.scope.element_id("tabs", &key),
				key: key.clone().into(),
				labels: tabs.labels.iter().cloned().map(Into::into).collect(),
				children: tabs.children.clone(),
				selected: tabs.selected,
				style: style.cloned(),
				row,
			}
			.into_any_element(),
			NodeKind::Section(section) => SectionView {
				ctx: self.clone(),
				id: self.scope.element_id("section", &key),
				title: section.title.clone().into(),
				collapsed: section.collapsed,
				children: section.children.clone(),
			}
			.into_any_element(),
			NodeKind::Popover(children) => PopoverView {
				ctx: self.clone(),
				id: self.scope.element_id("popover", &key),
				children: children.clone(),
			}
			.into_any_element(),
			NodeKind::Embed(card) => embed_card(card, self.scope.element_id("embed", &key), theme),
			NodeKind::UserChip(user) => UserBadge {
				user: Snowflake(*user),
				size: CHIP_AVATAR,
				name: true,
			}
			.into_any_element(),
			NodeKind::ChannelChip(channel) => ChannelBadge {
				channel: Snowflake(*channel),
			}
			.into_any_element(),
			NodeKind::Timestamp(timestamp) => TimestampLabel {
				unix: timestamp.unix_ms / 1000,
				style: markdown_style(&timestamp.style),
			}
			.into_any_element(),
		};
		match tip(style) {
			Some(text) if !matches!(node.kind, NodeKind::IconButton(_) | NodeKind::Button(_)) => div()
				.id(self.scope.element_id("tip", &key))
				.tooltip(tooltip(text))
				.child(element)
				.into_any_element(),
			_ => element,
		}
	}

	fn button(&self, button: &ButtonNode, key: &str, style: Option<&Style>, theme: &Theme) -> AnyElement {
		let scope = self.scope.clone();
		let key_owned = key.to_owned();
		let is_submit = button.submit;
		let on_click = move |event: &ClickEvent, _: &mut Window, cx: &mut App| {
			if matches!(
				scope.origin,
				Origin::Slot {
					slot: "composer-toolbar",
					..
				}
			) {
				remember_anchor(event.position(), cx);
			}
			if is_submit {
				submit(&scope, &key_owned, cx);
			} else {
				fire(&scope, &key_owned, Fire::Click, cx);
			}
		};
		let element = self.scope.element_id("button", key);
		if let Some(compact) = self.scope.compact {
			return compact_button(
				element,
				&button.label,
				button.icon.as_deref(),
				compact,
				disabled(style),
				theme,
				on_click,
			);
		}
		let mut widget = Button::new(element)
			.small()
			.label(button.label.clone())
			.disabled(disabled(style))
			.on_click(on_click);
		if let Some(path) = button.icon.as_deref().and_then(icon::resolve) {
			widget = widget.icon(gpui_component::Icon::default().path(path));
		}
		if let Some(text) = tip(style) {
			widget = widget.tooltip(text);
		}
		let widget = match (button.variant, button.tone) {
			(ButtonVariant::Ghost, _) => widget.ghost(),
			(ButtonVariant::Outline, _) => widget.outline(),
			(ButtonVariant::Link, _) => widget.link(),
			(ButtonVariant::Solid, Tone::Brand) => widget.primary(),
			(ButtonVariant::Solid, Tone::Danger) => widget.danger(),
			(ButtonVariant::Solid, Tone::Success) => widget.success(),
			(ButtonVariant::Solid, Tone::Warning) => widget.warning(),
			(ButtonVariant::Solid, Tone::Normal | Tone::Muted) => widget,
		};
		widget.into_any_element()
	}
}

fn collect_fields(tree: &ViewTree, index: usize, out: &mut Vec<(String, String)>, depth: usize) {
	let Some(node) = tree.nodes.get(index) else {
		return;
	};
	if depth > 0
		&& let Some(key) = &node.key
	{
		let value = match &node.kind {
			NodeKind::Input(input) | NodeKind::Textarea(input) => Some(input.value.clone()),
			NodeKind::Toggle(check) | NodeKind::Checkbox(check) => Some(check.checked.to_string()),
			NodeKind::Select(select) => Some(select.selected.clone().unwrap_or_default()),
			NodeKind::Slider(slider) => Some(slider.value.to_string()),
			_ => None,
		};
		if let Some(value) = value {
			out.push((key.clone(), value));
		}
	}
	let children: &[u32] = match &node.kind {
		NodeKind::Stack(stack) => &stack.children,
		NodeKind::Scroll(children) | NodeKind::Form(children) | NodeKind::Popover(children) => children,
		NodeKind::Section(section) => &section.children,
		NodeKind::Tabs(tabs) => &tabs.children,
		_ => &[],
	};
	for child in children {
		collect_fields(tree, *child as usize, out, depth + 1);
	}
}

fn markdown_style(code: &str) -> TimestampStyle {
	match code {
		"t" => TimestampStyle::ShortTime,
		"T" => TimestampStyle::LongTime,
		"d" => TimestampStyle::ShortDate,
		"D" => TimestampStyle::LongDate,
		"F" => TimestampStyle::LongDateTime,
		"R" => TimestampStyle::Relative,
		_ => TimestampStyle::ShortDateTime,
	}
}

fn text_element(text: &TextNode, theme: &Theme) -> Div {
	let color = text
		.color
		.as_ref()
		.and_then(|color| resolve(color, theme))
		.unwrap_or_else(|| tone_color(text.tone, theme));
	div()
		.min_w_0()
		.text_size(text_size(text.size, theme))
		.text_color(color)
		.font_weight(weight(text.weight))
		.when(text.mono, |label| label.font_family(theme.code_font.clone()))
		.when(text.italic, |label| label.italic())
		.child(text.value.clone())
}

pub fn badge_element(label: &str, fill: Hsla, theme: &Theme) -> AnyElement {
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

fn link_element(label: &str, url: &str, id: SharedString, theme: &Theme) -> AnyElement {
	let url = url.to_owned();
	div()
		.id(id)
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

fn emoji_element(emoji: &EmojiRef, size: u32, theme: &Theme) -> AnyElement {
	let size = if size == 0 { 22 } else { size };
	if emoji.id.is_some() {
		return PluginImage {
			source: ImageSource::Emoji(emoji.clone()),
			width: size,
			height: size,
		}
		.into_any_element();
	}
	div()
		.text_size(rem(size as f32))
		.text_color(theme.text_normal)
		.child(emoji.name.clone())
		.into_any_element()
}

fn compact_button(
	element: SharedString,
	label: &str,
	icon_name: Option<&str>,
	compact: Compact,
	inactive: bool,
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
		.text_size(theme.font_xs)
		.font_weight(FontWeight::MEDIUM)
		.text_color(theme.interactive_normal)
		.when(inactive, |button| button.opacity(DISABLED_OPACITY))
		.when(!inactive, |button| {
			button
				.cursor_pointer()
				.hover(move |style| style.bg(hover).text_color(hover_text))
				.on_click(on_click)
		})
		.when_some(tip, |button, tip| button.tooltip(tooltip(tip)))
		.children(icon_name.filter(|_| has_icon).map(|name| {
			PluginIcon::new(name.to_owned())
				.size(rem(compact.icon))
				.color(theme.interactive_normal)
		}))
		.when(show_label, |button| button.child(label.to_owned()))
		.into_any_element()
}

struct IconButtonSpec {
	element: SharedString,
	color: Hsla,
	tip: Option<SharedString>,
	inactive: bool,
	size: f32,
}

fn icon_button(
	spec: IconButtonSpec,
	name: &str,
	theme: &Theme,
	on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
	let hover = theme.modifier_hover;
	div()
		.id(spec.element)
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size(rem(spec.size + 8.))
		.rounded(theme.radius_sm)
		.when(spec.inactive, |button| button.opacity(DISABLED_OPACITY))
		.when(!spec.inactive, |button| {
			button
				.cursor_pointer()
				.hover(move |style| style.bg(hover))
				.on_click(on_click)
		})
		.when_some(spec.tip, |button, tip| button.tooltip(tooltip(tip)))
		.child(
			PluginIcon::new(name.to_owned())
				.size(rem(spec.size))
				.color(spec.color),
		)
		.into_any_element()
}

pub fn embed_card(card: &EmbedCard, id: SharedString, theme: &Theme) -> AnyElement {
	let accent = card
		.accent
		.as_ref()
		.and_then(|color| resolve(color, theme))
		.unwrap_or(theme.divider);
	let title = card.title.clone().map(|title| match &card.url {
		Some(url) => link_element(&title, url, id.clone(), theme),
		None => div()
			.text_size(theme.font_md)
			.font_weight(FontWeight::SEMIBOLD)
			.text_color(theme.header_primary)
			.child(title)
			.into_any_element(),
	});
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.max_w(rem(EMBED_MAX_WIDTH))
		.mt(theme.space_4)
		.p(theme.space_12)
		.rounded(theme.radius_sm)
		.border_l_4()
		.border_color(accent)
		.bg(theme.background_secondary)
		.children(title)
		.children(card.description.as_deref().map(|text| markdown_text(text, theme)))
		.children(card.image.clone().map(|url| PluginImage {
			source: ImageSource::Url(url),
			width: EMBED_IMAGE,
			height: EMBED_IMAGE / 2,
		}))
		.children(card.footer.clone().map(|footer| {
			div()
				.text_size(theme.font_tiny)
				.text_color(theme.text_muted)
				.child(footer)
		}))
		.into_any_element()
}

#[derive(IntoElement)]
struct PluginImage {
	source: ImageSource,
	width: u32,
	height: u32,
}

impl RenderOnce for PluginImage {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let (width, height) = match (self.width, self.height) {
			(0, 0) => (IMAGE_DEFAULT, IMAGE_DEFAULT),
			(0, height) => (height, height),
			(width, 0) => (width, width),
			pair => pair,
		};
		let (width, height) = (width.min(IMAGE_MAX), height.min(IMAGE_MAX));
		let (url, round) = match &self.source {
			ImageSource::Url(url) => (url.clone(), false),
			ImageSource::Emoji(emoji) => match emoji.id {
				Some(id) => (media::cdn::emoji_url(id, emoji.animated, width * 2), false),
				None => return div().child(emoji.name.clone()).into_any_element(),
			},
			ImageSource::Avatar(user) => {
				let data = cx.global::<PluginBridge>().data().read(cx);
				(avatar_for(&data.state(), Snowflake(*user), width * 2), true)
			}
		};
		let image = cx.global::<Images>().get(&url, SizeHint::new(width, height));
		div()
			.flex_none()
			.w(rem(width as f32))
			.h(rem(height as f32))
			.overflow_hidden()
			.when(round, |frame| frame.rounded_full())
			.when(!round, |frame| frame.rounded(theme.radius_sm))
			.children(image.map(|image| {
				img(image)
					.id(frame_id(&url))
					.size_full()
					.object_fit(ObjectFit::Contain)
			}))
			.into_any_element()
	}
}

#[derive(IntoElement)]
struct UserBadge {
	user: Snowflake,
	size: f32,
	name: bool,
}

impl RenderOnce for UserBadge {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let (display, url) = {
			let data = cx.global::<PluginBridge>().data().read(cx);
			let state = data.state();
			let guild = state.selected().and_then(|selected| selected.guild_id);
			(
				state.display_name(guild, self.user),
				avatar_for(&state, self.user, (self.size * 2.) as u32),
			)
		};
		let avatar = Avatar::new(display.clone()).size(rem(self.size)).url(url);
		if !self.name {
			return avatar.into_any_element();
		}
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_4)
			.px(theme.space_4)
			.rounded(theme.radius_sm)
			.bg(theme.mention_background)
			.text_color(theme.mention_text)
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.child(avatar)
			.child(display)
			.into_any_element()
	}
}

#[derive(IntoElement)]
struct ChannelBadge {
	channel: Snowflake,
}

impl RenderOnce for ChannelBadge {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let name = cx
			.global::<PluginBridge>()
			.data()
			.read(cx)
			.state()
			.channel(self.channel)
			.and_then(|channel| channel.name.clone())
			.unwrap_or_else(|| self.channel.to_string());
		div()
			.flex_none()
			.px(theme.space_4)
			.rounded(theme.radius_sm)
			.bg(theme.mention_background)
			.text_color(theme.mention_text)
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.child(format!("#{name}"))
	}
}

#[derive(IntoElement)]
struct TimestampLabel {
	unix: i64,
	style: TimestampStyle,
}

impl RenderOnce for TimestampLabel {
	fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		div()
			.flex_none()
			.px(theme.space_2)
			.rounded(theme.radius_sm)
			.bg(theme.code_background)
			.text_size(theme.font_sm)
			.child(format::timestamp(self.unix, self.style, cx))
	}
}

struct Synced<T> {
	declared: T,
	current: T,
}

impl<T: Clone + PartialEq> Synced<T> {
	fn new(value: T) -> Self {
		Self {
			declared: value.clone(),
			current: value,
		}
	}

	fn sync(&mut self, value: &T) -> bool {
		if self.declared == *value {
			return false;
		}
		self.declared = value.clone();
		self.current = value.clone();
		true
	}
}

fn synced<T: Clone + PartialEq + 'static>(
	id: SharedString,
	value: T,
	window: &mut Window,
	cx: &mut App,
) -> Entity<Synced<T>> {
	let initial = value.clone();
	let state = window.use_keyed_state(id, cx, move |_, _| Synced::new(initial));
	state.update(cx, |state, cx| {
		if state.sync(&value) {
			cx.notify();
		}
	});
	state
}

enum FieldState {
	Single(Entity<InputState>),
	Multi(Entity<TextareaState>),
}

impl FieldState {
	fn value(&self, cx: &App) -> SharedString {
		match self {
			Self::Single(input) => input.read(cx).value(),
			Self::Multi(input) => input.read(cx).value(),
		}
	}

	fn set(&self, value: SharedString, window: &mut Window, cx: &mut App) {
		match self {
			Self::Single(input) => input.update(cx, |state, cx| state.set_value(value, window, cx)),
			Self::Multi(input) => input.update(cx, |state, cx| state.set_value(value, window, cx)),
		}
	}
}

struct FieldBinding {
	field: FieldState,
	declared: SharedString,
	_subscription: Subscription,
}

struct FieldSpec {
	scope: Scope,
	key: SharedString,
	value: SharedString,
	placeholder: SharedString,
	multiline: bool,
}

impl FieldBinding {
	fn new(spec: FieldSpec, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let FieldSpec {
			scope,
			key,
			value,
			placeholder,
			multiline,
		} = spec;
		let (field, subscription) = if multiline {
			let input = cx.new(|cx| {
				let mut state = TextareaState::new(window, cx)
					.auto_grow(3, 8)
					.placeholder(placeholder.clone());
				state.set_value(value.clone(), window, cx);
				state
			});
			let subscription = Self::listen(&input, scope, key, window, cx);
			(FieldState::Multi(input), subscription)
		} else {
			let input = cx.new(|cx| {
				let mut state = InputState::new(window, cx).placeholder(placeholder.clone());
				state.set_value(value.clone(), window, cx);
				state
			});
			let subscription = Self::listen(&input, scope, key, window, cx);
			(FieldState::Single(input), subscription)
		};
		Self {
			field,
			declared: value,
			_subscription: subscription,
		}
	}

	fn listen<E: gpui::EventEmitter<InputEvent>>(
		input: &Entity<E>,
		scope: Scope,
		key: SharedString,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Subscription {
		cx.subscribe_in(input, window, move |this, _, event: &InputEvent, window, cx| {
			let text = this.field.value(cx);
			match event {
				InputEvent::Change => {
					if let Some(form) = &scope.form {
						cx.global::<PluginBridge>()
							.record_field(&form.id, &key, text.to_string());
					} else if scope.legacy() {
						note_input(&scope.plugin, &key, text, cx);
					}
				}
				InputEvent::PressEnter { .. } if scope.form.is_some() => {
					if matches!(this.field, FieldState::Single(_)) {
						submit(&scope, &key, cx);
					}
				}
				InputEvent::PressEnter { .. } => {
					if this.declared.is_empty() {
						this.field.set(SharedString::default(), window, cx);
					}
					if scope.legacy() {
						forget_input(&scope.plugin, &key, cx);
					}
					fire(&scope, &key, Fire::Change(text.to_string()), cx);
				}
				InputEvent::Blur if scope.form.is_none() => {
					if scope.legacy() {
						forget_input(&scope.plugin, &key, cx);
						fire(&scope, &key, Fire::Change(text.to_string()), cx);
					} else if text != this.declared {
						fire(&scope, &key, Fire::Change(text.to_string()), cx);
					}
				}
				_ => {}
			}
		})
	}

	fn sync(&mut self, value: &SharedString, window: &mut Window, cx: &mut App) {
		if self.declared != *value {
			self.declared = value.clone();
			self.field.set(value.clone(), window, cx);
		}
	}
}

#[derive(IntoElement)]
struct FieldInput {
	scope: Scope,
	key: SharedString,
	placeholder: SharedString,
	value: SharedString,
	multiline: bool,
	disabled: bool,
}

impl RenderOnce for FieldInput {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let id = self.scope.field_id("input", &self.key, cx);
		let spec = FieldSpec {
			scope: self.scope.clone(),
			key: self.key.clone(),
			value: self.value.clone(),
			placeholder: self.placeholder.clone(),
			multiline: self.multiline,
		};
		let binding = window.use_keyed_state(id, cx, move |window, cx| FieldBinding::new(spec, window, cx));
		binding.update(cx, |binding, cx| binding.sync(&self.value, window, cx));
		let field = match &binding.read(cx).field {
			FieldState::Single(input) => Input::new(input)
				.appearance(false)
				.bordered(false)
				.focus_bordered(false)
				.disabled(self.disabled)
				.small()
				.w_full()
				.into_any_element(),
			FieldState::Multi(input) => Textarea::new(input)
				.appearance(false)
				.bordered(false)
				.disabled(self.disabled)
				.into_any_element(),
		};
		div()
			.flex()
			.items_center()
			.w_full()
			.min_w(rem(INPUT_MIN_WIDTH))
			.when(!self.multiline, |frame| frame.h(rem(INPUT_HEIGHT)))
			.when(self.multiline, |frame| frame.py(theme.space_8))
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.bg(theme.input_background)
			.child(field)
	}
}

#[derive(IntoElement)]
struct CheckField {
	scope: Scope,
	key: SharedString,
	label: SharedString,
	checked: bool,
	switch: bool,
	disabled: bool,
}

impl RenderOnce for CheckField {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let id = self.scope.field_id("check", &self.key, cx);
		let state = synced(id.clone(), self.checked, window, cx);
		let checked = state.read(cx).current;
		let (scope, key) = (self.scope, self.key);
		let on_click = move |on: &bool, _: &mut Window, cx: &mut App| {
			let on = *on;
			state.update(cx, |state, cx| {
				state.current = on;
				cx.notify();
			});
			change(&scope, &key, on.to_string(), Fire::Toggle(on), cx);
		};
		if self.switch {
			Switch::new(id)
				.checked(checked)
				.disabled(self.disabled)
				.label(self.label)
				.on_click(on_click)
				.into_any_element()
		} else {
			Checkbox::new(id)
				.checked(checked)
				.disabled(self.disabled)
				.label(self.label)
				.on_click(on_click)
				.into_any_element()
		}
	}
}

#[derive(IntoElement)]
struct SelectField {
	scope: Scope,
	key: SharedString,
	options: Vec<(SharedString, SharedString)>,
	selected: Option<SharedString>,
	placeholder: SharedString,
}

impl RenderOnce for SelectField {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let id = self.scope.field_id("select", &self.key, cx);
		let state = synced(id.clone(), self.selected.clone(), window, cx);
		let current = state.read(cx).current.clone();
		let hover = theme.modifier_hover;
		let chips: Vec<AnyElement> = self
			.options
			.into_iter()
			.enumerate()
			.map(|(index, (value, label))| {
				let active = current.as_ref() == Some(&value);
				let (scope, key, state) = (self.scope.clone(), self.key.clone(), state.clone());
				div()
					.id((id.clone(), index))
					.px(theme.space_8)
					.h(rem(BADGE_HEIGHT + 6.))
					.flex()
					.items_center()
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.text_size(theme.font_sm)
					.when(active, |chip| {
						chip.bg(theme.brand).text_color(theme.text_on_brand)
					})
					.when(!active, |chip| {
						chip.bg(theme.background_secondary)
							.text_color(theme.text_normal)
							.hover(move |style| style.bg(hover))
					})
					.on_click(move |_, _, cx| {
						state.update(cx, |state, cx| {
							state.current = Some(value.clone());
							cx.notify();
						});
						change(
							&scope,
							&key,
							value.to_string(),
							Fire::Select(value.to_string()),
							cx,
						);
					})
					.child(label)
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_4)
			.when(current.is_none() && !self.placeholder.is_empty(), |field| {
				field.child(
					div()
						.text_size(theme.font_xs)
						.text_color(theme.text_muted)
						.child(self.placeholder.clone()),
				)
			})
			.child(div().flex().flex_wrap().gap(theme.space_4).children(chips))
	}
}

#[derive(IntoElement)]
struct SliderField {
	scope: Scope,
	key: SharedString,
	min: f32,
	max: f32,
	step: f32,
	value: f32,
}

impl RenderOnce for SliderField {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let id = self.scope.field_id("slider", &self.key, cx);
		let state = synced(id.clone(), self.value, window, cx);
		let current = state.read(cx).current;
		let fraction = (current - self.min) / (self.max - self.min);
		let hover = theme.modifier_hover;
		let stepper = |label: &'static str, delta: f32| {
			let (scope, key, state) = (self.scope.clone(), self.key.clone(), state.clone());
			let (min, max) = (self.min, self.max);
			div()
				.id((id.clone(), usize::from(delta > 0.)))
				.flex()
				.items_center()
				.justify_center()
				.size(rem(INPUT_HEIGHT - 8.))
				.rounded(theme.radius_sm)
				.cursor_pointer()
				.bg(theme.background_secondary)
				.hover(move |style| style.bg(hover))
				.on_click(move |_, _, cx| {
					let next = (state.read(cx).current + delta).clamp(min, max);
					state.update(cx, |state, cx| {
						state.current = next;
						cx.notify();
					});
					change(&scope, &key, next.to_string(), Fire::Change(next.to_string()), cx);
				})
				.child(label)
		};
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.child(stepper("-", -self.step))
			.child(
				div()
					.w(rem(PROGRESS_WIDTH))
					.child(Progress::new(id.clone()).value(fraction.clamp(0., 1.) * 100.)),
			)
			.child(stepper("+", self.step))
			.child(
				div()
					.min_w(rem(32.))
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.child(current.to_string()),
			)
	}
}

#[derive(IntoElement)]
struct TabsView {
	ctx: Ctx,
	id: SharedString,
	key: SharedString,
	labels: Vec<SharedString>,
	children: Vec<u32>,
	selected: u32,
	style: Option<Style>,
	row: bool,
}

impl RenderOnce for TabsView {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let state = synced(self.id.clone(), self.selected, window, cx);
		let last = self.children.len().saturating_sub(1) as u32;
		let selected = state.read(cx).current.min(last);
		let hover = theme.interactive_hover;
		let headers: Vec<AnyElement> = self
			.labels
			.iter()
			.enumerate()
			.map(|(index, label)| {
				let index = index as u32;
				let active = index == selected;
				let (state, scope, key) = (state.clone(), self.ctx.scope.clone(), self.key.clone());
				div()
					.id((self.id.clone(), index as usize))
					.px(theme.space_8)
					.pb(theme.space_4)
					.cursor_pointer()
					.text_size(theme.font_md)
					.font_weight(FontWeight::MEDIUM)
					.border_b_2()
					.when(active, |tab| {
						tab.border_color(theme.brand).text_color(theme.header_primary)
					})
					.when(!active, |tab| {
						tab.border_color(gpui::transparent_black())
							.text_color(theme.interactive_normal)
							.hover(move |style| style.text_color(hover))
					})
					.on_click(move |_, _, cx| {
						state.update(cx, |state, cx| {
							state.current = index;
							cx.notify();
						});
						if !scope.legacy() {
							fire(&scope, &key, Fire::Tab(index), cx);
						}
					})
					.child(label.clone())
					.into_any_element()
			})
			.collect();
		let body = self
			.children
			.get(selected as usize)
			.map(|child| self.ctx.node(*child as usize, false, &theme));
		styled(
			div().flex().flex_col().gap(theme.space_12),
			self.style.as_ref(),
			self.row,
			&theme,
		)
		.child(
			div()
				.flex()
				.gap(theme.space_8)
				.border_b_1()
				.border_color(theme.divider)
				.children(headers),
		)
		.children(body)
	}
}

#[derive(IntoElement)]
struct SectionView {
	ctx: Ctx,
	id: SharedString,
	title: SharedString,
	collapsed: bool,
	children: Vec<u32>,
}

impl RenderOnce for SectionView {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let state = synced(self.id.clone(), self.collapsed, window, cx);
		let collapsed = state.read(cx).current;
		let header = SectionHeader::new(self.id.clone(), self.title.clone())
			.collapsible(collapsed)
			.on_click(move |_, _, cx| {
				state.update(cx, |state, cx| {
					state.current = !state.current;
					cx.notify();
				});
			});
		div()
			.flex()
			.flex_col()
			.w_full()
			.child(header)
			.when(!collapsed, |section| {
				section.child(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_4)
						.px(theme.space_8)
						.children(self.ctx.children(&self.children, false, &theme)),
				)
			})
	}
}

#[derive(IntoElement)]
struct PopoverView {
	ctx: Ctx,
	id: SharedString,
	children: Vec<u32>,
}

impl RenderOnce for PopoverView {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let theme = cx.global::<Theme>().clone();
		let state = synced(self.id.clone(), false, window, cx);
		let open = state.read(cx).current;
		let Some((trigger, content)) = self.children.split_first() else {
			return div().into_any_element();
		};
		let toggle = state.clone();
		div()
			.relative()
			.flex_none()
			.child(
				div()
					.id(self.id.clone())
					.cursor_pointer()
					.on_mouse_down(MouseButton::Left, move |_, _, cx| {
						toggle.update(cx, |state, cx| {
							state.current = !state.current;
							cx.notify();
						});
						cx.stop_propagation();
					})
					.child(self.ctx.node(*trigger as usize, true, &theme)),
			)
			.when(open, |frame| {
				frame.child(deferred(
					anchored().snap_to_window().child(
						div()
							.occlude()
							.mt(theme.space_4)
							.min_w(rem(POPOVER_MIN_WIDTH))
							.max_w(rem(POPOVER_MAX_WIDTH))
							.flex()
							.flex_col()
							.gap(theme.space_8)
							.p(theme.space_12)
							.rounded(theme.radius_md)
							.border_1()
							.border_color(theme.divider)
							.bg(theme.background_floating)
							.on_mouse_down_out(move |_, _, cx| {
								state.update(cx, |state, cx| {
									state.current = false;
									cx.notify();
								});
							})
							.children(self.ctx.children(content, false, &theme)),
					),
				))
			})
			.into_any_element()
	}
}
