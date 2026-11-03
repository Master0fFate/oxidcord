use gpui::{AnyElement, App, FontWeight, IntoElement as _, ParentElement as _, Styled as _, div};
use gpui_component::button::{Button, ButtonVariants as _};
use protocol::{Message, MessageActivity, Snowflake};
use session::{Command, SecretKind};

use crate::i18n::Translate as _;
use crate::shell::community::data_handle;
use crate::theme::Theme;

pub fn card(message: &Message, me: Option<Snowflake>, theme: &Theme, cx: &App) -> Option<AnyElement> {
	let activity = message
		.activity
		.as_ref()
		.filter(|activity| activity.kind == Some(MessageActivity::JOIN))?;
	let session_id = activity
		.session_id
		.clone()
		.filter(|session| !session.is_empty())?;
	let application_id = message
		.application_id
		.or_else(|| message.application.as_ref().map(|application| application.id))?;
	if me == Some(message.author.id) {
		return None;
	}
	let (user_id, channel_id, message_id) = (message.author.id, message.channel_id, message.id);
	let name = message
		.application
		.as_ref()
		.and_then(|application| application.name.clone())
		.unwrap_or_default();
	Some(
		div()
			.flex()
			.items_center()
			.gap(theme.space_16)
			.mt(theme.space_4)
			.p(theme.space_16)
			.w(theme.invite_card_width)
			.max_w_full()
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_secondary)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap(theme.space_4)
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(cx.t("game-invite-caption").to_uppercase()),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_primary)
							.child(name),
					),
			)
			.child(
				Button::new(("game-invite-join", message_id.get() as usize))
					.success()
					.label(cx.t("activity-join"))
					.on_click(move |_, _, cx| {
						data_handle(cx).read(cx).send(Command::JoinActivity {
							user_id,
							session_id: session_id.clone(),
							application_id,
							kind: SecretKind::Join,
							invite: Some((channel_id, message_id)),
						});
					}),
			)
			.into_any_element(),
	)
}
