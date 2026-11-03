use std::time::Duration;

use gpui::{
	Animation, AnimationExt as _, AnyElement, App, FontWeight, IntoElement as _, ParentElement as _,
	Styled as _, div, pulsating_between,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use protocol::MessageFlags;
use state::{InteractionPhase, MessageView};

use super::{RowAction, RowEnv, reply_line};
use crate::components::Avatar;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::lookup::user_avatar;
use crate::theme::{rem, with_alpha};

const PULSE: Duration = Duration::from_millis(1200);

pub fn header(view: &MessageView, env: &RowEnv, cx: &App) -> Option<AnyElement> {
	let theme = env.theme;
	let (user, label) = view.message.invoked_command()?;
	let name = env.lookup.user(user.id.get());
	Some(reply_line(
		div()
			.flex()
			.items_center()
			.gap(theme.space_4)
			.min_w_0()
			.child(
				Avatar::new(name.clone())
					.size(rem(16.))
					.url(user_avatar(user, 32)),
			)
			.child(
				div()
					.flex_none()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_secondary)
					.child(format!("@{name}")),
			)
			.child(
				div()
					.flex_none()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t("message-interaction-used")),
			)
			.child(
				div()
					.truncate()
					.px(theme.space_4)
					.rounded(theme.radius_sm)
					.bg(with_alpha(theme.brand, 0.16))
					.text_size(theme.font_sm)
					.font_weight(FontWeight::MEDIUM)
					.text_color(theme.interactive_active)
					.child(label),
			)
			.into_any_element(),
		Vec::new(),
		theme,
	))
}

pub fn status(view: &MessageView, env: &RowEnv, cx: &App) -> Vec<AnyElement> {
	let theme = env.theme;
	let id = view.message.id.get();
	let loading = view.message.flags.contains(MessageFlags::LOADING);
	let phase = view.interaction;
	let mut lines = Vec::new();
	if loading || phase == Some(InteractionPhase::Loading) {
		lines.push(
			div()
				.italic()
				.text_size(theme.font_sm)
				.text_color(theme.text_muted)
				.child(cx.t(if loading {
					"message-interaction-thinking"
				} else {
					"message-interaction-loading"
				}))
				.with_animation(
					("interaction-pulse", id),
					Animation::new(PULSE)
						.repeat()
						.with_easing(pulsating_between(0.4, 1.)),
					|line, delta| line.opacity(delta),
				)
				.into_any_element(),
		);
	}
	if phase == Some(InteractionPhase::Failed) {
		lines.push(
			div()
				.text_size(theme.font_sm)
				.text_color(theme.danger)
				.child(cx.t("message-interaction-failed"))
				.into_any_element(),
		);
	}
	lines
}

pub fn ephemeral_notice(view: &MessageView, env: &RowEnv, cx: &App) -> Option<AnyElement> {
	let message = &view.message;
	if !message.flags.contains(MessageFlags::EPHEMERAL) {
		return None;
	}
	let theme = env.theme;
	let id = message.id;
	let dispatch = env.dispatch.clone();
	Some(
		div()
			.flex()
			.items_center()
			.gap(theme.space_4)
			.text_size(theme.font_sm)
			.text_color(theme.text_muted)
			.child(Icon::new(IconKind::Eye).size(rem(14.)).color(theme.text_muted))
			.child(cx.t("message-ephemeral"))
			.child(
				Button::new(("message-ephemeral-dismiss", id.get()))
					.small()
					.ghost()
					.label(cx.t("message-dismiss"))
					.on_click(move |_, window, cx| dispatch(RowAction::DismissEphemeral(id), window, cx)),
			)
			.into_any_element(),
	)
}
