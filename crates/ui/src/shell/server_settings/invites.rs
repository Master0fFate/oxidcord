use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement,
	ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use session::Command;
use state::InviteInfo;

use super::kit;
use crate::components::{
	Avatar, IconButton, PickHandler, ToggleHandler, badge, dialog_footer, field, select, switch_row,
};
use crate::data::{Data, DataEvent};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::lookup::avatar_for;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const AGES: [(&str, u32); 7] = [
	("invite-age-30-minutes", 1800),
	("invite-age-1-hour", 3600),
	("invite-age-6-hours", 21_600),
	("invite-age-12-hours", 43_200),
	("invite-age-1-day", 86_400),
	("invite-age-7-days", 604_800),
	("invite-age-never", 0),
];
const USES: [(&str, u32); 7] = [
	("invite-uses-unlimited", 0),
	("invite-uses-1", 1),
	("invite-uses-5", 5),
	("invite-uses-10", 10),
	("invite-uses-25", 25),
	("invite-uses-50", 50),
	("invite-uses-100", 100),
];
const DEFAULT_AGE: usize = 5;

pub fn invite_link(code: &str) -> String {
	format!("https://discord.gg/{code}")
}

fn confirm_revoke(window: &mut Window, cx: &mut App, guild: Snowflake, code: String) {
	let data = crate::shell::community::data_handle(cx);
	window.open_alert_dialog(cx, move |alert, _, cx| {
		let data = data.clone();
		let code = code.clone();
		alert
			.title(cx.t("invite-revoke-title"))
			.description(cx.t_args("invite-revoke-body", &[("code", ArgValue::from(code.clone()))]))
			.confirm()
			.ok_text(cx.t("invite-revoke"))
			.ok_variant(ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				data.read(cx).send(Command::RevokeInvite {
					guild_id: guild,
					code: code.clone(),
				});
				true
			})
	});
}

pub struct InvitesPage {
	data: Entity<Data>,
	guild: Snowflake,
	_subscription: Subscription,
}

impl InvitesPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadInvites { guild_id: guild });
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			if matches!(event, DataEvent::Changed(_)) {
				cx.notify();
			}
		});
		Self {
			data,
			guild,
			_subscription: subscription,
		}
	}

	fn row(&self, invite: &InviteInfo, theme: &Theme, cx: &App) -> AnyElement {
		let state = self.data.read(cx).state();
		let inviter = invite.inviter_id.map(|id| {
			(
				state.display_name(Some(self.guild), id),
				avatar_for(&state, id, 48),
			)
		});
		let channel = invite
			.channel_id
			.and_then(|id| state.channel(id).and_then(|channel| channel.name.clone()))
			.map(|name| format!("#{name}"));
		drop(state);
		let uses = if invite.max_uses > 0 {
			format!("{} / {}", invite.uses, invite.max_uses)
		} else {
			invite.uses.to_string()
		};
		let expires: SharedString = match invite.expires_at {
			Some(at) => format::message_time(at, cx),
			None => cx.t("invite-age-never"),
		};
		let link = invite_link(&invite.code);
		let code = invite.code.clone();
		let revoke_code = invite.code.clone();
		let guild = self.guild;
		div()
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.list_row_height)
			.px(theme.space_12)
			.border_b_1()
			.border_color(theme.divider)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.w(theme.manage_grid_cell * 2.)
					.children(inviter.map(|(name, avatar)| {
						div()
							.flex()
							.items_center()
							.gap(theme.space_8)
							.min_w_0()
							.child(Avatar::new(name.clone()).size(theme.portrait_sm).url(avatar))
							.child(
								div()
									.truncate()
									.text_size(theme.font_sm)
									.text_color(theme.text_normal)
									.child(name),
							)
					})),
			)
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_size(theme.font_md)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(code.clone()),
			)
			.children(channel.map(|name| {
				div()
					.w(theme.manage_grid_cell)
					.truncate()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(name)
			}))
			.child(
				div()
					.w(theme.picker_rail)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(uses),
			)
			.child(
				div()
					.w(theme.manage_grid_cell)
					.truncate()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(expires),
			)
			.when(invite.temporary, |row| {
				row.child(badge(cx.t("invite-temporary"), theme.warning, theme))
			})
			.child(
				IconButton::new(SharedString::from(format!("invite-copy-{code}")), IconKind::Copy)
					.tooltip(cx.t("invite-copy"))
					.on_click(move |_, _, _| {
						if let Err(error) = platform::clipboard::write_text(&link) {
							tracing::warn!(%error, "could not copy the invite link");
						}
					}),
			)
			.child(
				IconButton::new(
					SharedString::from(format!("invite-revoke-{code}")),
					IconKind::Trash,
				)
				.color(theme.danger)
				.tooltip(cx.t("invite-revoke"))
				.on_click(move |_, window, cx| confirm_revoke(window, cx, guild, revoke_code.clone())),
			)
			.into_any_element()
	}
}

impl Render for InvitesPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let invites: Vec<InviteInfo> = self.data.read(cx).state().guild_invites(self.guild).to_vec();
		let rows: Vec<AnyElement> = invites
			.iter()
			.map(|invite| self.row(invite, &theme, cx))
			.collect();
		let empty = rows.is_empty();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-invites"), &theme))
			.child(kit::count_caption(
				cx.t_args("invites-count", &[("count", ArgValue::from(invites.len()))]),
				&theme,
			))
			.child(
				div()
					.flex()
					.flex_col()
					.pb(theme.space_32)
					.children(rows)
					.when(empty, |list| {
						list.child(kit::empty(IconKind::Mail, cx.t("invites-empty"), &theme))
					})
					.overflow_y_scrollbar()
					.id("invites-list")
					.flex_1()
					.min_h_0(),
			)
	}
}

pub struct CreateInvite {
	data: Entity<Data>,
	channel: Snowflake,
	age: usize,
	uses: usize,
	temporary: bool,
	requested: bool,
	received: bool,
	_subscription: Subscription,
}

impl EventEmitter<Close> for CreateInvite {}

pub fn open_create(window: &mut Window, cx: &mut App, channel: Snowflake) {
	let data = crate::shell::community::data_handle(cx);
	let view = cx.new(|cx| {
		let subscription = cx.subscribe(&data, move |this: &mut CreateInvite, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event {
				if this.requested && changes.loaded.contains(channel) {
					this.received = true;
				}
				cx.notify();
			}
		});
		CreateInvite {
			data: data.clone(),
			channel,
			age: DEFAULT_AGE,
			uses: 0,
			temporary: false,
			requested: false,
			received: false,
			_subscription: subscription,
		}
	});
	let title = cx.t("invite-create-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, None);
}

impl CreateInvite {
	fn generate(&mut self, cx: &mut Context<Self>) {
		self.requested = true;
		self.received = false;
		self.data.read(cx).send(Command::CreateInvite {
			channel_id: self.channel,
			max_age: AGES[self.age].1,
			max_uses: USES[self.uses].1,
			temporary: self.temporary,
		});
		cx.notify();
	}

	fn created(&self, cx: &App) -> Option<InviteInfo> {
		if !self.received {
			return None;
		}
		self.data.read(cx).state().invite_created(self.channel).cloned()
	}
}

impl Render for CreateInvite {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let weak = cx.weak_entity();
		let age_labels: Vec<SharedString> = AGES.iter().map(|(key, _)| cx.t(key)).collect();
		let use_labels: Vec<SharedString> = USES.iter().map(|(key, _)| cx.t(key)).collect();
		let pick_age: PickHandler = {
			let weak = weak.clone();
			Rc::new(move |index, _, cx| {
				weak.update(cx, |this, cx| {
					this.age = index;
					this.received = false;
					cx.notify();
				})
				.ok();
			})
		};
		let pick_uses: PickHandler = {
			let weak = weak.clone();
			Rc::new(move |index, _, cx| {
				weak.update(cx, |this, cx| {
					this.uses = index;
					this.received = false;
					cx.notify();
				})
				.ok();
			})
		};
		let temporary: ToggleHandler = {
			let weak = weak.clone();
			Rc::new(move |value, _, cx| {
				weak.update(cx, |this, cx| {
					this.temporary = value;
					this.received = false;
					cx.notify();
				})
				.ok();
			})
		};
		let created = self.created(cx);
		let weak_close = weak.clone();
		let weak_generate = weak;
		let link_row = created.as_ref().map(|invite| {
			let link = invite_link(&invite.code);
			let copy = link.clone();
			field(
				cx.t("invite-link"),
				&theme,
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.px(theme.space_12)
							.h(theme.control_height + theme.space_4)
							.flex()
							.items_center()
							.rounded(theme.radius_sm)
							.bg(theme.input_background)
							.text_size(theme.font_md)
							.text_color(theme.text_normal)
							.child(link),
					)
					.child(
						Button::new("invite-copy-link")
							.primary()
							.label(cx.t("invite-copy"))
							.on_click(move |_, _, _| {
								if let Err(error) = platform::clipboard::write_text(&copy) {
									tracing::warn!(%error, "could not copy the invite link");
								}
							}),
					),
			)
		});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(
				cx.t("invite-expire-after"),
				&theme,
				select(
					"invite-age",
					age_labels[self.age].clone(),
					&age_labels,
					self.age,
					pick_age,
				),
			))
			.child(field(
				cx.t("invite-max-uses"),
				&theme,
				select(
					"invite-uses",
					use_labels[self.uses].clone(),
					&use_labels,
					self.uses,
					pick_uses,
				),
			))
			.child(switch_row(
				"invite-temporary",
				cx.t("invite-temporary-title"),
				Some(cx.t("invite-temporary-hint")),
				self.temporary,
				&theme,
				temporary,
			))
			.children(link_row)
			.child(dialog_footer(
				cx.t("action-close"),
				cx.t(if created.is_some() {
					"invite-regenerate"
				} else {
					"invite-generate"
				}),
				false,
				true,
				&theme,
				move |_, cx| {
					weak_close.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_generate.update(cx, |this, cx| this.generate(cx)).ok();
				},
			))
	}
}
