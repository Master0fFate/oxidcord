use gpui::{
	AnyElement, App, ClickEvent, FontWeight, IntoElement, ParentElement as _, Styled as _, Window, div,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use protocol::{JoinRequestStatus, Snowflake};

use super::community::screening;
use crate::i18n::Translate as _;
use crate::theme::Theme;

#[derive(Clone, PartialEq, Eq)]
pub struct Membership {
	pub guild_id: Snowflake,
	pub status: Option<JoinRequestStatus>,
}

pub fn membership_bar(membership: &Membership, theme: &Theme, cx: &App) -> AnyElement {
	let guild_id = membership.guild_id;
	let (title, completable) = match membership.status {
		Some(JoinRequestStatus::Submitted) => ("membership-bar-submitted", false),
		Some(JoinRequestStatus::Rejected) => ("membership-bar-rejected", false),
		_ => ("membership-bar-title", true),
	};
	div()
		.flex()
		.flex_none()
		.items_center()
		.justify_between()
		.gap(theme.space_12)
		.px(theme.space_16)
		.py(theme.space_8)
		.border_t_1()
		.border_color(theme.divider)
		.bg(theme.background_secondary)
		.child(
			div()
				.truncate()
				.text_size(theme.font_md)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(theme.header_primary)
				.child(cx.t(title)),
		)
		.children(completable.then(|| {
			Button::new("membership-complete")
				.small()
				.primary()
				.label(cx.t("membership-bar-complete"))
				.on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
					screening::open(window, cx, guild_id);
				})
		}))
		.into_any_element()
}
