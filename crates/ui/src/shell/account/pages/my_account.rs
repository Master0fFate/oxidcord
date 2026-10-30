use gpui::{
	Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render, SharedString,
	Styled as _, Subscription, Window, div,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use session::Command;

use super::widgets::{heading, row, section};
use super::{PageEvent, Target};
use crate::components::Avatar;
use crate::data::{Data, DataEvent, streamer_mode};
use crate::i18n::Translate as _;
use crate::lookup::user_avatar;
use crate::model::{Status, role_color};
use crate::shell::account::avatar_ring;
use crate::theme::Theme;

const WEB_APP: &str = "https://discord.com/channels/@me";

pub struct MyAccountPage {
	data: Entity<Data>,
	_subscription: Subscription,
}

impl EventEmitter<PageEvent> for MyAccountPage {}

fn mask_email(email: &str) -> String {
	match email.split_once('@') {
		Some((local, domain)) => {
			let first: String = local.chars().take(1).collect();
			format!(
				"{first}{}@{domain}",
				"\u{2022}".repeat(local.chars().count().saturating_sub(1).max(3))
			)
		}
		None => "\u{2022}".repeat(email.chars().count().min(12)),
	}
}

impl MyAccountPage {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.current_user
			{
				cx.notify();
			}
		});
		if let Some(me) = data.read(cx).me() {
			data.read(cx).send(Command::LoadProfile {
				user_id: me,
				guild_id: None,
			});
		}
		Self {
			data,
			_subscription: subscription,
		}
	}
}

fn open_web_app() {
	if let Err(error) = platform::open::open_url(WEB_APP) {
		tracing::warn!(%error, "could not open the web app");
	}
}

impl Render for MyAccountPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (name, username, email, avatar, accent, status) = {
			let data = self.data.read(cx);
			let state = data.state();
			let Some(user) = state.current_user() else {
				return div().into_any_element();
			};
			let accent = user
				.accent_color
				.and_then(|color| u32::try_from(color).ok())
				.map_or(theme.brand, role_color);
			(
				SharedString::from(state.display_name(None, user.id)),
				SharedString::from(user.username.clone()),
				user.email.as_deref().map(mask_email).unwrap_or_default(),
				user_avatar(user, 256),
				accent,
				crate::model::status_of(&state, user.id),
			)
		};
		let email = if streamer_mode(cx).hides_personal() {
			"\u{2022}".repeat(email.chars().count().min(12))
		} else {
			email
		};
		let cutout = theme.background_secondary;
		let web_note = cx.t("account-web-note");
		let open_button = |id: &'static str, cx: &Context<Self>| {
			Button::new(id)
				.small()
				.label(cx.t("account-open-web"))
				.on_click(|_, _, _| open_web_app())
		};
		let card = div()
			.flex()
			.flex_col()
			.rounded(theme.radius_md)
			.bg(theme.background_secondary)
			.overflow_hidden()
			.child(div().h(theme.profile_banner_height_lg).w_full().bg(accent))
			.child(
				div()
					.flex()
					.items_end()
					.gap(theme.space_16)
					.px(theme.space_16)
					.mt(-(theme.avatar_lg / 2.))
					.child(
						avatar_ring(&theme, cutout, theme.avatar_lg, theme.space_4).child(
							Avatar::new(name.clone())
								.size(theme.avatar_lg)
								.url(avatar)
								.status(if status == Status::Offline {
									Status::Online
								} else {
									status
								})
								.cutout(cutout),
						),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.pb(theme.space_8)
							.text_size(theme.font_xl)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_primary)
							.truncate()
							.child(name.clone()),
					)
					.child(
						div().pb(theme.space_8).child(
							Button::new("account-edit-profile")
								.primary()
								.label(cx.t("account-edit-profile"))
								.on_click(
									cx.listener(|_, _, _, cx| cx.emit(PageEvent::Goto(Target::Profiles))),
								),
						),
					),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.m(theme.space_16)
					.px(theme.space_16)
					.rounded(theme.radius_md)
					.bg(theme.background_tertiary)
					.child(row(cx.t("account-display-name"), Some(name), div(), &theme))
					.child(row(cx.t("account-username"), Some(username), div(), &theme))
					.child(row(cx.t("account-email"), Some(email.into()), div(), &theme)),
			);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(card)
			.child(section(
				cx.t("account-security"),
				vec![
					row(
						cx.t("account-password"),
						Some(web_note.clone()),
						open_button("account-open-password", cx),
						&theme,
					),
					row(
						cx.t("account-two-factor"),
						Some(web_note),
						open_button("account-open-2fa", cx),
						&theme,
					),
				],
				&theme,
			))
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(heading(cx.t("account-sign-out"), &theme))
					.child(
						div().child(
							Button::new("account-log-out")
								.danger()
								.label(cx.t("settings-log-out"))
								.on_click(cx.listener(|_, _, _, cx| cx.emit(PageEvent::LogOut))),
						),
					),
			)
			.into_any_element()
	}
}
