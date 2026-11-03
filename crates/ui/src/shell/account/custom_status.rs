use chrono::{DateTime, Duration, Local, TimeDelta, Utc};
use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dialog::DialogFooter;
use gpui_component::input::{Input, InputState};
use protocol::{ProtoCustomStatus, Snowflake};
use session::Command;

use crate::components::StatusEmoji;
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::emoji_picker::{EmojiChoice, EmojiPicker, EmojiPickerEvent};
use crate::theme::Theme;

const MAX_LENGTH: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClearAfter {
	Never,
	ThirtyMinutes,
	OneHour,
	FourHours,
	Today,
	Keep(u64),
}

impl ClearAfter {
	fn kept(expires_at_ms: Option<u64>) -> Self {
		expires_at_ms.map_or(Self::Never, Self::Keep)
	}

	const ALL: [ClearAfter; 5] = [
		Self::Today,
		Self::FourHours,
		Self::OneHour,
		Self::ThirtyMinutes,
		Self::Never,
	];

	fn key(self) -> &'static str {
		match self {
			Self::Never => "custom-status-never",
			Self::ThirtyMinutes => "custom-status-30-minutes",
			Self::OneHour => "custom-status-1-hour",
			Self::FourHours => "custom-status-4-hours",
			Self::Today | Self::Keep(_) => "custom-status-today",
		}
	}

	fn expires(self) -> Option<DateTime<Utc>> {
		let now = Utc::now();
		match self {
			Self::Never => None,
			Self::ThirtyMinutes => Some(now + Duration::minutes(30)),
			Self::OneHour => Some(now + Duration::hours(1)),
			Self::FourHours => Some(now + Duration::hours(4)),
			Self::Keep(at_ms) => DateTime::from_timestamp_millis(i64::try_from(at_ms).ok()?),
			Self::Today => {
				let tomorrow = Local::now().date_naive().succ_opt()?;
				let midnight = tomorrow.and_hms_opt(0, 0, 0)?;
				let local = midnight.and_local_timezone(Local).earliest()?;
				Some(local.with_timezone(&Utc)).filter(|end| *end - now > TimeDelta::zero())
			}
		}
	}
}

fn saved_emoji(saved: &ProtoCustomStatus) -> Option<EmojiChoice> {
	match (saved.emoji_id, &saved.emoji_name) {
		(Some(id), name) => Some(EmojiChoice::Custom {
			name: name.clone().unwrap_or_default(),
			id: id.get(),
			animated: false,
		}),
		(None, Some(name)) => Some(EmojiChoice::Unicode(name.clone())),
		(None, None) => None,
	}
}

fn preview(choice: &EmojiChoice) -> StatusEmoji {
	match choice {
		EmojiChoice::Unicode(glyph) => StatusEmoji::Unicode(glyph.clone().into()),
		EmojiChoice::Custom { id, animated, .. } => StatusEmoji::Custom {
			id: *id,
			animated: *animated,
		},
	}
}

struct CustomStatusForm {
	data: Entity<Data>,
	input: Entity<InputState>,
	emoji: Option<EmojiChoice>,
	clear_after: ClearAfter,
	picker: Entity<EmojiPicker>,
	picker_open: bool,
	_subscription: Subscription,
}

impl CustomStatusForm {
	fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let saved = data.read(cx).state().custom_status().cloned();
		let placeholder = cx.t("custom-status-placeholder");
		let input = cx.new(|cx| {
			let mut input = InputState::new(window, cx).placeholder(placeholder);
			if let Some(saved) = &saved {
				input.set_value(saved.text.clone(), window, cx);
			}
			input
		});
		let picker = cx.new(|cx| EmojiPicker::new(window, cx));
		picker.update(cx, |picker, cx| picker.open(Vec::new(), window, cx));
		let subscription = cx.subscribe(&picker, |this, _, event: &EmojiPickerEvent, cx| {
			let EmojiPickerEvent::Pick(choice) = event;
			this.emoji = Some(choice.clone());
			this.picker_open = false;
			cx.notify();
		});
		Self {
			data,
			input,
			emoji: saved.as_ref().and_then(saved_emoji),
			clear_after: saved
				.as_ref()
				.map_or(ClearAfter::Today, |saved| ClearAfter::kept(saved.expires_at_ms)),
			picker,
			picker_open: false,
			_subscription: subscription,
		}
	}

	fn save(&self, cx: &App) {
		let text: String = self
			.input
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(MAX_LENGTH)
			.collect();
		let text = (!text.is_empty()).then_some(text);
		let has_content = text.is_some() || self.emoji.is_some();
		let (emoji, emoji_id) = match &self.emoji {
			Some(EmojiChoice::Unicode(glyph)) => (Some(glyph.clone()), None),
			Some(EmojiChoice::Custom { name, id, .. }) => (Some(name.clone()), Some(Snowflake(*id))),
			None => (None, None),
		};
		self.data.read(cx).send(Command::SetCustomStatus {
			text,
			emoji,
			emoji_id,
			expires: has_content.then(|| self.clear_after.expires()).flatten(),
		});
	}

	fn clear_choice(&self, choice: ClearAfter, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let selected = self.clear_after == choice;
		div()
			.id(choice.key())
			.flex()
			.items_center()
			.h(theme.avatar_sm)
			.px(theme.space_12)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.when(selected, |chip| {
				chip.bg(theme.brand).text_color(theme.text_on_brand)
			})
			.when(!selected, |chip| {
				chip.bg(theme.background_tertiary)
					.text_color(theme.interactive_normal)
					.hover(|style| style.bg(theme.modifier_selected))
			})
			.on_click(cx.listener(move |this, _, _, cx| {
				this.clear_after = choice;
				cx.notify();
			}))
			.child(cx.t(choice.key()))
			.into_any_element()
	}
}

impl Render for CustomStatusForm {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let chosen = self.emoji.as_ref().map(preview);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(cx.t("custom-status-label").to_uppercase()),
					)
					.child(
						div()
							.flex()
							.items_center()
							.gap(theme.space_8)
							.child(
								div()
									.id("custom-status-emoji")
									.flex()
									.flex_none()
									.items_center()
									.justify_center()
									.size(theme.field_height)
									.rounded(theme.radius_sm)
									.bg(theme.input_background)
									.cursor_pointer()
									.hover(|style| style.bg(theme.modifier_selected))
									.on_click(cx.listener(|this, _, _, cx| {
										this.picker_open = !this.picker_open;
										cx.notify();
									}))
									.child(match &chosen {
										Some(emoji) => emoji.element(theme.icon_lg, cx),
										None => Icon::new(IconKind::Emoji)
											.size(theme.icon_lg)
											.color(theme.interactive_normal)
											.into_any_element(),
									}),
							)
							.child(div().flex_1().child(Input::new(&self.input).cleanable(true))),
					),
			)
			.when(self.picker_open, |form| form.child(self.picker.clone()))
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(cx.t("custom-status-clear-after").to_uppercase()),
					)
					.child(
						div().flex().flex_wrap().gap(theme.space_8).children(
							ClearAfter::ALL
								.into_iter()
								.map(|choice| self.clear_choice(choice, &theme, cx)),
						),
					),
			)
	}
}

pub fn open(data: Entity<Data>, window: &mut Window, cx: &mut App) {
	let form = cx.new(|cx| CustomStatusForm::new(data.clone(), window, cx));
	window.open_dialog(cx, move |dialog, window, cx| {
		let save = form.clone();
		let clear = data.clone();
		dialog
			.title(cx.t("custom-status-title"))
			.w(cx.global::<Theme>().modal_width.to_pixels(window.rem_size()))
			.child(form.clone())
			.footer(
				DialogFooter::new()
					.child(
						Button::new("custom-status-clear")
							.ghost()
							.label(cx.t("custom-status-clear"))
							.on_click(move |_, window, cx| {
								clear.read(cx).send(Command::SetCustomStatus {
									text: None,
									emoji: None,
									emoji_id: None,
									expires: None,
								});
								window.close_dialog(cx);
							}),
					)
					.child(
						Button::new("custom-status-cancel")
							.label(cx.t("action-cancel"))
							.on_click(|_, window, cx| window.close_dialog(cx)),
					)
					.child(
						Button::new("custom-status-save")
							.primary()
							.label(cx.t("custom-status-save"))
							.on_click(move |_, window, cx| {
								save.read(cx).save(cx);
								window.close_dialog(cx);
							}),
					),
			)
	});
}
