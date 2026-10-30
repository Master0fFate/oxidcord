use std::rc::Rc;

use gpui::{
	AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, MouseButton, ObjectFit,
	ParentElement as _, StatefulInteractiveElement as _, Styled as _, StyledImage as _, div, img,
	prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use media::SizeHint;
use protocol::Snowflake;

use super::activity::{ActivityView, OpenLink, activity_card_full};
use super::profile_data::ProfileData;
use super::{AccountHub, parts, rich};
use crate::components::{Avatar, IconButton};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::images::Images;
use crate::theme::Theme;

const ICON_PIXELS: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FullTab {
	Info,
	Servers,
	Friends,
}

pub(super) struct FullProfile {
	pub user: Snowflake,
	pub guild: Option<Snowflake>,
	pub tab: FullTab,
}

impl FullTab {
	fn key(self) -> &'static str {
		match self {
			Self::Info => "profile-tab-info",
			Self::Servers => "profile-tab-servers",
			Self::Friends => "profile-tab-friends",
		}
	}
}

impl AccountHub {
	fn tab_button(
		&self,
		tab: FullTab,
		count: Option<usize>,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let selected = self.full.as_ref().is_some_and(|full| full.tab == tab);
		let label = match count {
			Some(count) => cx.t_args(
				"profile-tab-count",
				&[
					("name", ArgValue::from(cx.t(tab.key()).to_string())),
					("count", ArgValue::from(count)),
				],
			),
			None => cx.t(tab.key()),
		};
		div()
			.id(tab.key())
			.px(theme.space_4)
			.pb(theme.space_8)
			.cursor_pointer()
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.border_b_2()
			.border_color(if selected {
				theme.brand
			} else {
				gpui::transparent_black()
			})
			.text_color(if selected {
				theme.interactive_active
			} else {
				theme.interactive_normal
			})
			.on_click(cx.listener(move |this, _, _, cx| {
				if let Some(full) = this.full.as_mut() {
					full.tab = tab;
					cx.notify();
				}
			}))
			.child(label)
			.into_any_element()
	}

	fn info_tab(
		&self,
		profile: &ProfileData,
		activities: &[ActivityView],
		open: &OpenLink,
		theme: &Theme,
		cx: &Context<Self>,
	) -> AnyElement {
		let mut column = div().flex().flex_col().gap(theme.space_16);
		if !activities.is_empty() {
			let now = chrono::Utc::now().timestamp_millis();
			column = column.child(
				div().flex().flex_col().gap(theme.space_8).children(
					activities
						.iter()
						.enumerate()
						.map(|(index, activity)| activity_card_full(activity, index, now, open, theme, cx)),
				),
			);
		}
		if let Some(bio) = profile.bio.as_deref() {
			column = column.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_4)
					.child(parts::section_label(cx.t("profile-about-me"), theme))
					.child(rich::markdown_text(bio, theme)),
			);
		}
		column = column.child(
			div()
				.flex()
				.flex_col()
				.gap(theme.space_4)
				.child(parts::section_label(cx.t("profile-member-since"), theme))
				.child(
					div()
						.flex()
						.flex_wrap()
						.gap(theme.space_12)
						.text_size(theme.font_sm)
						.child(format::day_label(profile.created))
						.when_some(profile.joined, |line, joined| {
							line.child(div().text_color(theme.text_muted).child(cx.t_args(
								"profile-joined-server",
								&[("date", ArgValue::from(format::day_label(joined)))],
							)))
						}),
				),
		);
		if !profile.roles.is_empty() {
			column = column.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(parts::section_label(cx.t("profile-roles"), theme))
					.child(
						div()
							.flex()
							.flex_wrap()
							.gap(theme.space_4)
							.children(profile.roles.iter().map(|role| parts::role_chip(role, theme))),
					),
			);
		}
		if !profile.connections.is_empty() {
			column = column.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_4)
					.child(parts::section_label(cx.t("profile-connections"), theme))
					.children(profile.connections.iter().map(|(kind, name)| {
						div()
							.flex()
							.gap(theme.space_8)
							.text_size(theme.font_sm)
							.child(div().text_color(theme.text_muted).child(kind.clone()))
							.child(name.clone())
					})),
			);
		}
		column.into_any_element()
	}

	fn servers_tab(&self, profile: &ProfileData, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		if profile.mutual_guilds.is_empty() {
			return empty(cx.t("profile-no-mutual-servers"), theme);
		}
		div()
			.flex()
			.flex_col()
			.gap(theme.space_4)
			.children(profile.mutual_guilds.iter().map(|guild| {
				let icon = guild.icon.as_ref().and_then(|hash| {
					cx.global::<Images>().get(
						&media::cdn::guild_icon_url(guild.id.get(), hash, ICON_PIXELS),
						SizeHint::square(ICON_PIXELS),
					)
				});
				div()
					.flex()
					.items_center()
					.gap(theme.space_12)
					.p(theme.space_8)
					.rounded(theme.radius_sm)
					.hover(|style| style.bg(theme.modifier_hover))
					.child(
						div()
							.flex()
							.flex_none()
							.items_center()
							.justify_center()
							.size(theme.avatar_md)
							.rounded_full()
							.overflow_hidden()
							.bg(theme.background_tertiary)
							.text_size(theme.font_sm)
							.font_weight(FontWeight::SEMIBOLD)
							.map(|icon_box| match icon {
								Some(icon) => {
									icon_box.child(img(icon).size_full().object_fit(ObjectFit::Cover))
								}
								None => icon_box
									.child(guild.name.chars().next().map(String::from).unwrap_or_default()),
							}),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.child(guild.name.clone()),
					)
			}))
			.into_any_element()
	}

	fn friends_tab(&self, profile: &ProfileData, theme: &Theme, cx: &Context<Self>) -> AnyElement {
		if profile.mutual_friends.is_empty() {
			return empty(cx.t("profile-no-mutual-friends"), theme);
		}
		div()
			.flex()
			.flex_col()
			.gap(theme.space_4)
			.children(profile.mutual_friends.iter().map(|friend| {
				div()
					.flex()
					.items_center()
					.gap(theme.space_12)
					.p(theme.space_8)
					.rounded(theme.radius_sm)
					.hover(|style| style.bg(theme.modifier_hover))
					.child(
						Avatar::new(friend.name.clone())
							.size(theme.avatar_md)
							.url(friend.avatar.clone())
							.cutout(theme.background_floating),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.child(friend.name.clone()),
					)
			}))
			.into_any_element()
	}

	pub(super) fn full_profile_card(
		&self,
		profile: &ProfileData,
		activities: &[ActivityView],
		open: &OpenLink,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let weak = cx.weak_entity();
		let close: parts::CloseFn = Rc::new(move |_, cx| {
			weak.update(cx, |this, cx| {
				this.full = None;
				cx.notify();
			})
			.ok();
		});
		let tab = self.full.as_ref().map_or(FullTab::Info, |full| full.tab);
		let body = match tab {
			FullTab::Info => self.info_tab(profile, activities, open, theme, cx),
			FullTab::Servers => self.servers_tab(profile, theme, cx),
			FullTab::Friends => self.friends_tab(profile, theme, cx),
		};
		let cutout = theme.background_floating;
		let handle = match &profile.pronouns {
			Some(pronouns) => format!("{} \u{b7} {pronouns}", profile.handle),
			None => profile.handle.to_string(),
		};
		let tabs = div()
			.flex()
			.gap(theme.space_16)
			.border_b_1()
			.border_color(theme.divider)
			.child(self.tab_button(FullTab::Info, None, theme, cx))
			.child(self.tab_button(FullTab::Servers, Some(profile.mutual_guilds.len()), theme, cx))
			.child(self.tab_button(
				FullTab::Friends,
				Some(profile.mutual_friend_count.max(profile.mutual_friends.len())),
				theme,
				cx,
			));
		div()
			.id("full-profile")
			.relative()
			.flex()
			.flex_col()
			.w(theme.profile_modal_width)
			.max_h_full()
			.rounded(theme.radius_md)
			.bg(theme.background_floating)
			.overflow_hidden()
			.text_color(theme.text_normal)
			.occlude()
			.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
			.child(parts::banner(profile, theme.profile_banner_height_lg, theme, cx))
			.child(
				div().absolute().top(theme.space_8).right(theme.space_8).child(
					IconButton::new("full-profile-close", IconKind::Close)
						.filled_hover()
						.hit_size(theme.avatar_sm)
						.color(theme.text_on_brand)
						.on_click(cx.listener(|this, _, _, cx| {
							this.full = None;
							cx.notify();
						})),
				),
			)
			.child(
				div()
					.flex()
					.items_end()
					.justify_between()
					.px(theme.space_16)
					.mt(-(theme.avatar_xl / 2.))
					.child(
						parts::avatar_ring(theme, cutout, theme.avatar_xl, theme.space_8).child(
							Avatar::new(profile.name.clone())
								.size(theme.avatar_xl)
								.url(profile.avatar.clone())
								.status(profile.status)
								.cutout(cutout),
						),
					)
					.child(
						div()
							.pb(theme.space_8)
							.child(parts::action_row(profile, &self.data, &close, theme, cx)),
					),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_16)
					.p(theme.space_16)
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.flex()
									.flex_col()
									.child(
										div()
											.text_size(theme.font_xl)
											.font_weight(FontWeight::BOLD)
											.text_color(theme.header_primary)
											.child(profile.name.clone()),
									)
									.child(
										div()
											.text_size(theme.font_sm)
											.text_color(theme.header_secondary)
											.child(handle),
									),
							)
							.child(parts::badges_row(&profile.badges, theme, cx)),
					)
					.child(tabs)
					.child(
						div()
							.h(theme.list_max_height)
							.child(body)
							.overflow_y_scrollbar()
							.id("full-profile-body"),
					),
			)
			.into_any_element()
	}
}

fn empty(text: gpui::SharedString, theme: &Theme) -> AnyElement {
	div()
		.p(theme.space_16)
		.text_size(theme.font_sm)
		.text_color(theme.text_muted)
		.child(text)
		.into_any_element()
}
