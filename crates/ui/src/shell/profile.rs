use std::rc::Rc;

use gpui::{
	Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render, Styled as _,
	Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use plugin_host::ViewContext;
use protocol::Snowflake;
use session::Command;

use super::account::{self, ProfileData};
use crate::components::{Avatar, Divider};
use crate::data::{Data, DataEvent};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::Theme;

pub enum ProfileEvent {
	Close,
	OpenFull {
		user: Snowflake,
		guild: Option<Snowflake>,
	},
}

pub struct ProfileCard {
	data: Entity<Data>,
	user: Snowflake,
	guild: Option<Snowflake>,
	_subscription: Subscription,
}

impl EventEmitter<ProfileEvent> for ProfileCard {}

impl ProfileCard {
	pub fn new(
		data: Entity<Data>,
		user: Snowflake,
		guild: Option<Snowflake>,
		cx: &mut Context<Self>,
	) -> Self {
		data.read(cx).send(Command::LoadProfile {
			user_id: user,
			guild_id: guild,
		});
		let subscription = cx.subscribe(&data, move |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.loaded.contains(user)
					|| changes.relationships
					|| changes.plugin_slots
					|| !changes.presence.is_empty())
			{
				cx.notify();
			}
		});
		Self {
			data,
			user,
			guild,
			_subscription: subscription,
		}
	}
}

impl Render for ProfileCard {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let profile = {
			let data = self.data.read(cx);
			ProfileData::load(&data.state(), self.user, self.guild)
		};
		let Some(profile) = profile else {
			return div().into_any_element();
		};
		let weak = cx.weak_entity();
		let close: account::CloseFn = Rc::new(move |_, cx| {
			weak.update(cx, |_, cx| cx.emit(ProfileEvent::Close)).ok();
		});
		let (user, guild) = (self.user, self.guild);
		let cutout = theme.background_floating;
		let banner = account::banner(&profile, theme.profile_banner_height, &theme, cx);
		let badges = account::badges_row(&profile.badges, &theme, cx);
		let actions = account::action_row(&profile, &self.data, &close, &theme, cx);
		let stats = account::stat_line(&profile, &theme, cx);
		let roles: Vec<_> = profile
			.roles
			.iter()
			.map(|role| account::role_chip(role, &theme))
			.collect();
		let bio = profile
			.bio
			.as_deref()
			.map(|bio| account::markdown_text(bio, &theme));
		let joined = profile.joined.map(format::day_label);
		let created = format::day_label(profile.created);
		let plugin_sections = super::plugins::contextual(
			"user-popout-section",
			ViewContext {
				guild_id: self.guild.map(Snowflake::get),
				user_id: Some(self.user.get()),
				..ViewContext::default()
			},
			cx,
		);
		let handle = match &profile.pronouns {
			Some(pronouns) => format!("{} \u{b7} {pronouns}", profile.handle),
			None => profile.handle.to_string(),
		};
		div()
			.flex()
			.flex_col()
			.w(theme.profile_popover_width)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.overflow_hidden()
			.text_color(theme.text_normal)
			.child(banner)
			.child(
				div()
					.flex()
					.items_start()
					.justify_between()
					.px(theme.space_16)
					.mt(-(theme.avatar_lg / 2.))
					.child(
						account::avatar_ring(cutout, theme.avatar_lg, theme.space_4).child(
							Avatar::new(profile.name.clone())
								.size(theme.avatar_lg)
								.url(profile.avatar.clone())
								.status(profile.status)
								.cutout(cutout),
						),
					)
					.child(div().pt(theme.avatar_lg / 2. + theme.space_8).child(badges)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.p(theme.space_16)
					.child(
						div()
							.flex()
							.flex_col()
							.child(
								div()
									.text_size(theme.font_lg)
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
					.child(Divider::horizontal())
					.when_some(bio, |card, bio| {
						card.child(
							div()
								.flex()
								.flex_col()
								.gap(theme.space_4)
								.child(account::section_label(cx.t("profile-about-me"), &theme))
								.child(bio),
						)
					})
					.child(
						div()
							.flex()
							.flex_col()
							.gap(theme.space_4)
							.child(account::section_label(cx.t("profile-member-since"), &theme))
							.child(
								div()
									.flex()
									.flex_wrap()
									.gap(theme.space_12)
									.text_size(theme.font_sm)
									.child(created)
									.when_some(joined, |line, joined| {
										line.child(div().text_color(theme.text_muted).child(cx.t_args(
											"profile-joined-server",
											&[("date", ArgValue::from(joined))],
										)))
									}),
							),
					)
					.when(!roles.is_empty(), |card| {
						card.child(
							div()
								.flex()
								.flex_col()
								.gap(theme.space_8)
								.child(account::section_label(cx.t("profile-roles"), &theme))
								.child(div().flex().flex_wrap().gap(theme.space_4).children(roles)),
						)
					})
					.children(plugin_sections)
					.children(stats)
					.child(actions)
					.child(
						Button::new("profile-view-full")
							.small()
							.link()
							.label(cx.t("profile-view-full"))
							.on_click(cx.listener(move |_, _, _, cx| {
								cx.emit(ProfileEvent::OpenFull { user, guild });
								cx.emit(ProfileEvent::Close);
							})),
					),
			)
			.into_any_element()
	}
}
