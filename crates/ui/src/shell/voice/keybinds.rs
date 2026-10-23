use std::rc::Rc;

use gpui::{
	AnyElement, Context, FocusHandle, Focusable, FontWeight, InteractiveElement as _, IntoElement,
	KeyDownEvent, ParentElement as _, Render, SharedString, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use store::KeybindSettings;

use super::hotkeys::{Bind, VoiceHotkeys};
use crate::components::IconButton;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

pub struct KeybindsPage {
	backend: Rc<SettingsBackend>,
	binds: KeybindSettings,
	recording: Option<Bind>,
	failed: Option<Bind>,
	focus: FocusHandle,
}

impl Focusable for KeybindsPage {
	fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
		self.focus.clone()
	}
}

fn describe(binding: &str) -> String {
	binding
		.split('+')
		.map(|part| part.trim().to_uppercase())
		.collect::<Vec<_>>()
		.join(" + ")
}

impl KeybindsPage {
	pub fn new(backend: Rc<SettingsBackend>, cx: &mut Context<Self>) -> Self {
		let binds = (backend.read)().keybinds;
		Self {
			backend,
			binds,
			recording: None,
			failed: None,
			focus: cx.focus_handle(),
		}
	}

	fn start_recording(&mut self, bind: Bind, window: &mut Window, cx: &mut Context<Self>) {
		self.recording = Some(bind);
		self.failed = None;
		window.focus(&self.focus, cx);
		cx.notify();
	}

	fn assign(&mut self, bind: Bind, binding: Option<String>, cx: &mut Context<Self>) {
		match VoiceHotkeys::bind(bind, binding.as_deref(), cx) {
			Ok(()) => {
				self.failed = None;
				bind.set(&mut self.binds, binding.clone());
				(self.backend.write)(&move |settings| bind.set(&mut settings.keybinds, binding.clone()));
			}
			Err(error) => {
				tracing::warn!(%error, "could not bind the key");
				self.failed = Some(bind);
			}
		}
		cx.notify();
	}

	fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
		let Some(bind) = self.recording else {
			return;
		};
		cx.stop_propagation();
		let keystroke = &event.keystroke;
		self.recording = None;
		if keystroke.key == "escape" {
			cx.notify();
			return;
		}
		let mut parts: Vec<&str> = Vec::new();
		if keystroke.modifiers.control {
			parts.push("ctrl");
		}
		if keystroke.modifiers.alt {
			parts.push("alt");
		}
		if keystroke.modifiers.shift {
			parts.push("shift");
		}
		if keystroke.modifiers.platform {
			parts.push("super");
		}
		parts.push(&keystroke.key);
		let binding = parts.join("+");
		self.assign(bind, Some(binding), cx);
	}

	fn row(&self, bind: Bind, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let current = bind.get(&self.binds).map(describe);
		let recording = self.recording == Some(bind);
		let failed = self.failed == Some(bind);
		let chip: SharedString = match (&current, recording) {
			(_, true) => cx.t("voice-keybind-press"),
			(Some(text), false) => text.clone().into(),
			(None, false) => cx.t("voice-keybind-unset"),
		};
		div()
			.flex()
			.flex_col()
			.gap(theme.space_4)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_16)
					.p(theme.space_12)
					.rounded(theme.radius_md)
					.bg(theme.background_secondary)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::MEDIUM)
							.text_color(theme.header_primary)
							.child(cx.t(bind.label_key())),
					)
					.child(
						div()
							.flex_none()
							.min_w(theme.voice_menu_width * 0.6)
							.px(theme.space_12)
							.py(theme.space_4)
							.rounded(theme.radius_sm)
							.bg(theme.background_tertiary)
							.text_size(theme.font_sm)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(if recording || current.is_some() {
								theme.text_normal
							} else {
								theme.text_muted
							})
							.child(chip),
					)
					.child(
						Button::new(("voice-keybind-record", bind as usize))
							.when(recording, |button| button.primary())
							.label(cx.t("voice-keybind-record"))
							.on_click(
								cx.listener(move |this, _, window, cx| {
									this.start_recording(bind, window, cx)
								}),
							),
					)
					.child(
						IconButton::new(("voice-keybind-clear", bind as usize), IconKind::Close)
							.filled_hover()
							.tooltip(cx.t("voice-keybind-clear"))
							.on_click(cx.listener(move |this, _, _, cx| this.assign(bind, None, cx))),
					),
			)
			.when(failed, |row| {
				row.child(
					div()
						.text_size(theme.font_xs)
						.text_color(theme.danger)
						.child(cx.t("voice-keybind-invalid")),
				)
			})
			.into_any_element()
	}
}

impl Render for KeybindsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let mut rows = Vec::new();
		for bind in Bind::ALL {
			rows.push(self.row(bind, &theme, cx));
		}
		div()
			.id("voice-keybinds")
			.track_focus(&self.focus)
			.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)))
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.children(rows)
			.child(
				div()
					.text_size(theme.font_xs)
					.text_color(theme.text_muted)
					.child(cx.t("voice-keybind-hint")),
			)
	}
}
