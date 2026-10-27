use gpui::{
	AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
	StatefulInteractiveElement as _, Styled as _, div,
};
use gpui_component::scroll::ScrollableElement as _;

use super::{FriendsEvent, FriendsPage};
use crate::components::Avatar;
use crate::i18n::Translate as _;
use crate::model::Status;
use crate::theme::Theme;

const AVATAR_PIXELS: u32 = 64;

impl FriendsPage {
	pub(super) fn active_now(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let active: Vec<_> = self
			.friends
			.iter()
			.filter(|friend| friend.activity.is_some())
			.collect();
		let body = if active.is_empty() {
			div()
				.flex()
				.flex_col()
				.gap(theme.space_4)
				.p(theme.space_12)
				.child(
					div()
						.text_size(theme.font_md)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(theme.header_primary)
						.child(cx.t("friends-active-empty-title")),
				)
				.child(
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_muted)
						.child(cx.t("friends-active-empty-body")),
				)
				.into_any_element()
		} else {
			div()
				.flex()
				.flex_col()
				.gap(theme.space_4)
				.children(active.into_iter().map(|friend| {
					let user = friend.user_id;
					let name: SharedString = friend.display.clone().into();
					let activity: SharedString = friend.activity.clone().unwrap_or_default().into();
					let avatar =
						media::cdn::avatar_url(user.get(), friend.avatar.as_deref(), None, AVATAR_PIXELS);
					div()
						.id(("active-friend", user.get()))
						.flex()
						.items_center()
						.gap(theme.space_12)
						.p(theme.space_8)
						.rounded(theme.radius_sm)
						.cursor_pointer()
						.hover(|style| style.bg(theme.modifier_hover))
						.on_click(cx.listener(move |_, event: &gpui::ClickEvent, _, cx| {
							cx.emit(FriendsEvent::OpenProfile {
								user,
								at: event.position(),
							})
						}))
						.child(
							Avatar::new(name.clone())
								.size(theme.avatar_md)
								.url(avatar)
								.status(Status::from(&friend.status))
								.cutout(theme.background_primary),
						)
						.child(
							div()
								.flex()
								.flex_col()
								.min_w_0()
								.child(
									div()
										.truncate()
										.text_size(theme.font_md)
										.font_weight(FontWeight::SEMIBOLD)
										.text_color(theme.header_primary)
										.child(name),
								)
								.child(
									div()
										.truncate()
										.text_size(theme.font_sm)
										.text_color(theme.text_muted)
										.child(activity),
								),
						)
				}))
				.into_any_element()
		};
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.friends_active_width)
			.h_full()
			.border_l_1()
			.border_color(theme.divider)
			.p(theme.space_16)
			.gap(theme.space_16)
			.child(
				div()
					.text_size(theme.font_lg)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(cx.t("friends-active-title")),
			)
			.child(
				div()
					.flex_1()
					.min_h_0()
					.child(body)
					.overflow_y_scrollbar()
					.id("friends-active"),
			)
			.into_any_element()
	}
}
