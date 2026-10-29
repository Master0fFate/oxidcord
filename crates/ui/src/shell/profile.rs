use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
	Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render, Styled as _,
	Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use plugin_host::ViewContext;
use protocol::Snowflake;
use session::Command;

use super::account::{self, ProfileData};
use super::plugins::{self, LinkOrigin, MenuTarget, plugin_menu};
use super::{SettingsBackend, links};
use crate::components::{Avatar, Divider};
use crate::data::{Data, DataEvent};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::Theme;

const TICK: Duration = Duration::from_secs(1);

pub enum ProfileEvent {
	Close,
	OpenFull {
		user: Snowflake,
		guild: Option<Snowflake>,
	},
}

pub struct ProfileCard {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	user: Snowflake,
	guild: Option<Snowflake>,
	requested_icons: HashSet<Snowflake>,
	ticking: bool,
	_subscription: Subscription,
	_ticker: Task<()>,
}

impl EventEmitter<ProfileEvent> for ProfileCard {}

impl ProfileCard {
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		user: Snowflake,
		guild: Option<Snowflake>,
		cx: &mut Context<Self>,
	) -> Self {
		{
			let data = data.read(cx);
			data.send(Command::LoadProfile {
				user_id: user,
				guild_id: guild,
			});
			if data.state().current_user_id() != Some(user) {
				data.send(Command::RefreshPresence {
					user_id: user,
					guild_id: guild,
				});
			}
		}
		let subscription = cx.subscribe(&data, move |this, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.loaded.contains(user)
					|| changes.relationships
					|| changes.plugin_slots
					|| changes.presence.contains(user)
					|| this.requested_icons.iter().any(|id| changes.loaded.contains(*id)))
			{
				cx.notify();
			}
		});
		let ticker = cx.spawn(async move |this, cx| {
			loop {
				cx.background_executor().timer(TICK).await;
				let alive = this.update(cx, |this, cx| {
					if this.ticking {
						cx.notify();
					}
				});
				if alive.is_err() {
					return;
				}
			}
		});
		Self {
			data,
			backend,
			user,
			guild,
			requested_icons: HashSet::new(),
			ticking: false,
			_subscription: subscription,
			_ticker: ticker,
		}
	}
}

impl Render for ProfileCard {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (profile, activities) = {
			let data = self.data.read(cx);
			let state = data.state();
			(
				ProfileData::load(&state, self.user, self.guild),
				account::activities(&state, self.user),
			)
		};
		for application_id in activities.iter().filter_map(|activity| activity.missing_icon) {
			if self.requested_icons.insert(application_id) {
				self.data
					.read(cx)
					.send(Command::LoadApplicationIcon { application_id });
			}
		}
		self.ticking = activities.iter().any(|activity| activity.is_timed());
		let now = chrono::Utc::now().timestamp_millis();
		let Some(profile) = profile else {
			return div().into_any_element();
		};
		let (user, guild) = (self.user, self.guild);
		let backend = self.backend.clone();
		let open: account::OpenLink = Rc::new(move |url, window, cx| {
			links::open_link(url, LinkOrigin::default(), &backend, window, cx);
		});
		let deck = (!activities.is_empty()).then(|| {
			account::activity_deck(
				&activities,
				now,
				&open,
				&theme,
				cx,
				cx.listener(move |_, _, _, cx| {
					cx.emit(ProfileEvent::OpenFull { user, guild });
					cx.emit(ProfileEvent::Close);
				}),
			)
		});
		let weak = cx.weak_entity();
		let close: account::CloseFn = Rc::new(move |_, cx| {
			weak.update(cx, |_, cx| cx.emit(ProfileEvent::Close)).ok();
		});
		let cutout = theme.background_floating;
		let banner = account::banner(&profile, theme.profile_banner_height, &theme, cx);
		let plugin_badges = profile
			.over
			.as_ref()
			.map_or_else(Vec::new, |over| plugins::badges(over, self.user, &theme, cx));
		let decoration = plugins::decoration_of(profile.over.as_ref());
		let suffix = profile.over.as_ref().and_then(|over| over.suffix.clone());
		let badges = div()
			.flex()
			.items_center()
			.gap(theme.space_4)
			.child(account::badges_row(&profile.badges, &theme, cx))
			.children(plugin_badges);
		let actions = account::action_row(&profile, &self.data, &close, &theme, cx);
		let stats = account::stat_line(&profile, &theme, cx);
		let roles: Vec<_> = profile
			.roles
			.iter()
			.map(|role| {
				let chip = account::role_chip(role, &theme);
				match self.guild {
					Some(guild) => plugin_menu(
						("profile-role", role.id.get()),
						chip,
						"role-context-menu",
						MenuTarget::role(guild, role.id, role.own),
						cx,
					),
					None => chip,
				}
			})
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
								.decoration(decoration)
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
									.child(profile.name.clone())
									.when_some(suffix, |name, suffix| {
										name.child(
											div()
												.ml(theme.space_4)
												.text_size(theme.font_sm)
												.font_weight(FontWeight::NORMAL)
												.text_color(theme.text_muted)
												.child(suffix),
										)
									}),
							)
							.child(
								div()
									.text_size(theme.font_sm)
									.text_color(theme.header_secondary)
									.child(handle),
							),
					)
					.child(Divider::horizontal())
					.children(deck)
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
