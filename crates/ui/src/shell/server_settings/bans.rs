use gpui::{
	AnyElement, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
	Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use session::Command;
use state::BanEntry;

use super::kit;
use crate::components::Avatar;
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::lookup::user_avatar;
use crate::theme::Theme;

pub struct BansPage {
	data: Entity<Data>,
	guild: Snowflake,
	search: Entity<InputState>,
	_subscriptions: Vec<Subscription>,
}

impl BansPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadBans { guild_id: guild });
		let placeholder = cx.t("bans-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscriptions = vec![
			cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
				if matches!(event, DataEvent::Changed(_) | DataEvent::MediaReady) {
					cx.notify();
				}
			}),
			cx.subscribe_in(&search, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
		];
		Self {
			data,
			guild,
			search,
			_subscriptions: subscriptions,
		}
	}

	fn confirm_unban(&self, user: Snowflake, name: String, window: &mut Window, cx: &mut Context<Self>) {
		let data = self.data.clone();
		let guild = self.guild;
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			alert
				.title(cx.t_args("bans-unban-title", &[("name", ArgValue::from(name.clone()))]))
				.description(cx.t("bans-unban-body"))
				.confirm()
				.ok_text(cx.t("bans-unban"))
				.ok_variant(ButtonVariant::Primary)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::UnbanMember {
						guild_id: guild,
						user_id: user,
					});
					true
				})
		});
	}

	fn row(&self, ban: &BanEntry, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let user = ban.user.id;
		let name = ban
			.user
			.global_name
			.clone()
			.filter(|name| !name.is_empty())
			.unwrap_or_else(|| ban.user.username.clone());
		let unban_name = name.clone();
		div()
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.list_row_height + theme.space_8)
			.px(theme.space_12)
			.border_b_1()
			.border_color(theme.divider)
			.child(
				Avatar::new(name.clone())
					.size(theme.portrait_md)
					.url(user_avatar(&ban.user, 64)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
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
							.child(
								ban.reason
									.clone()
									.unwrap_or_else(|| cx.t("bans-no-reason").to_string()),
							),
					),
			)
			.child(
				Button::new(("ban-revoke", user.get()))
					.outline()
					.label(cx.t("bans-unban"))
					.on_click(cx.listener(move |this, _, window, cx| {
						this.confirm_unban(user, unban_name.clone(), window, cx)
					})),
			)
			.into_any_element()
	}
}

impl Render for BansPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let query = self.search.read(cx).value().trim().to_lowercase();
		let bans: Vec<BanEntry> = self
			.data
			.read(cx)
			.state()
			.bans(self.guild)
			.iter()
			.filter(|ban| {
				query.is_empty()
					|| ban.user.username.to_lowercase().contains(&query)
					|| ban
						.user
						.global_name
						.as_deref()
						.is_some_and(|name| name.to_lowercase().contains(&query))
					|| ban.user.id.to_string() == query
			})
			.cloned()
			.collect();
		let rows: Vec<AnyElement> = bans.iter().map(|ban| self.row(ban, &theme, cx)).collect();
		let empty = rows.is_empty();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-bans"), &theme))
			.child(
				div()
					.flex_none()
					.pb(theme.space_16)
					.child(Input::new(&self.search)),
			)
			.child(kit::count_caption(
				cx.t_args("bans-count", &[("count", ArgValue::from(bans.len()))]),
				&theme,
			))
			.child(
				div()
					.flex()
					.flex_col()
					.pb(theme.space_32)
					.children(rows)
					.when(empty, |list| {
						list.child(kit::empty(IconKind::Ban, cx.t("bans-empty"), &theme))
					})
					.overflow_y_scrollbar()
					.id("bans-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
