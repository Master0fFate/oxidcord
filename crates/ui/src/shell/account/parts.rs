use std::rc::Rc;

use gpui::{
	AnyElement, App, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement, ObjectFit,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Window,
	div, img, prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Disableable as _, Sizable as _};
use media::SizeHint;
use protocol::Snowflake;
use session::Command;

use super::profile_data::{Badge, ProfileData, Relation, RoleChip};
use crate::components::tooltip;
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::images::Images;
use crate::theme::{Theme, with_alpha};

pub type CloseFn = Rc<dyn Fn(&mut Window, &mut App)>;

const BADGE_PIXELS: u32 = 64;
const BANNER_PIXELS: u32 = 600;

pub fn section_label(label: SharedString, theme: &Theme) -> AnyElement {
	div()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(label.to_uppercase())
		.into_any_element()
}

pub fn role_chip(role: &RoleChip, theme: &Theme) -> AnyElement {
	let dot = role.color.unwrap_or(theme.interactive_normal);
	div()
		.flex()
		.items_center()
		.gap(theme.space_4 + theme.space_2)
		.h(theme.chip_height)
		.px(theme.space_8)
		.rounded(theme.radius_sm)
		.bg(with_alpha(theme.background_tertiary, 0.6))
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::MEDIUM)
		.child(
			div()
				.size(theme.space_12)
				.rounded(theme.round(theme.space_12))
				.bg(dot),
		)
		.child(role.name.clone())
		.into_any_element()
}

pub fn badges_row(badges: &[Badge], theme: &Theme, cx: &App) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_4)
		.children(badges.iter().enumerate().map(|(index, badge)| {
			let image = cx
				.global::<Images>()
				.get(&badge.icon_url, SizeHint::square(BADGE_PIXELS));
			div()
				.id(("profile-badge", index))
				.size(theme.badge_icon)
				.tooltip(tooltip(badge.description.clone()))
				.children(image.map(|image| img(image).size_full().object_fit(ObjectFit::Contain)))
		}))
		.into_any_element()
}

pub fn banner(profile: &ProfileData, height: gpui::Rems, theme: &Theme, cx: &App) -> AnyElement {
	let image = profile.banner_url.as_ref().and_then(|url| {
		cx.global::<Images>()
			.get(url, SizeHint::new(BANNER_PIXELS, BANNER_PIXELS / 2))
	});
	div()
		.h(height)
		.w_full()
		.flex_none()
		.bg(profile.accent.unwrap_or(theme.brand))
		.overflow_hidden()
		.children(image.map(|image| img(image).size_full().object_fit(ObjectFit::Cover)))
		.into_any_element()
}

fn copy_id(id: Snowflake) {
	if let Err(error) = platform::clipboard::write_text(&id.to_string()) {
		tracing::warn!(%error, "could not copy the user id");
	}
}

pub fn action_row(
	profile: &ProfileData,
	data: &Entity<Data>,
	close: &CloseFn,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	let id = profile.id;
	let username = profile.username.clone();
	let mut row = div().flex().flex_wrap().items_center().gap(theme.space_8);
	if !profile.mine {
		let message = {
			let (data, close) = (data.clone(), close.clone());
			move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
				data.read(cx).send(Command::OpenDm { user_id: id });
				close(window, cx);
			}
		};
		row = row.child(
			Button::new("profile-message")
				.primary()
				.small()
				.label(cx.t("profile-message"))
				.on_click(message),
		);
		if !profile.bot {
			let friend = {
				let data = data.clone();
				move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
					data.read(cx).send(Command::AddFriend {
						username: username.clone(),
					});
				}
			};
			row = match profile.relation {
				Relation::None => row.child(
					Button::new("profile-add-friend")
						.small()
						.label(cx.t("profile-add-friend"))
						.on_click(friend),
				),
				Relation::Outgoing => row.child(
					Button::new("profile-request-sent")
						.small()
						.disabled(true)
						.label(cx.t("profile-request-sent")),
				),
				Relation::Incoming => {
					let data = data.clone();
					row.child(
						Button::new("profile-accept-friend")
							.small()
							.label(cx.t("friends-accept"))
							.on_click(move |_, _, cx| {
								data.read(cx).send(Command::AcceptFriend { user_id: id });
							}),
					)
				}
				Relation::Friend | Relation::Blocked => row,
			};
		}
		if profile.relation != Relation::Blocked {
			let data = data.clone();
			row = row.child(
				Button::new("profile-block")
					.small()
					.ghost()
					.label(cx.t("profile-block"))
					.on_click(move |_, _, cx| {
						data.read(cx).send(Command::BlockUser { user_id: id });
					}),
			);
		}
	}
	row.child(
		Button::new("profile-copy-id")
			.small()
			.ghost()
			.label(cx.t("profile-copy-id"))
			.on_click(move |_, _, _| copy_id(id)),
	)
	.into_any_element()
}

pub fn stat_line(profile: &ProfileData, theme: &Theme, cx: &App) -> Option<AnyElement> {
	let servers = profile.mutual_guilds.len();
	let friends = profile.mutual_friend_count;
	if servers == 0 && friends == 0 {
		return None;
	}
	let count = |key: &str, value: usize| cx.t_args(key, &[("count", crate::i18n::ArgValue::from(value))]);
	Some(
		div()
			.flex()
			.flex_wrap()
			.gap(theme.space_12)
			.text_size(theme.font_xs)
			.text_color(theme.header_secondary)
			.when(servers > 0, |line| {
				line.child(count("profile-mutual-servers", servers))
			})
			.when(friends > 0, |line| {
				line.child(count("profile-mutual-friends", friends))
			})
			.into_any_element(),
	)
}

pub fn avatar_ring(theme: &Theme, color: Hsla, inner: gpui::Rems, ring: gpui::Rems) -> gpui::Div {
	div()
		.size(inner + ring * 2.)
		.rounded(theme.round(inner + ring * 2.))
		.bg(color)
		.p(ring)
		.flex_none()
}
