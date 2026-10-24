use gpui::{
	AnyElement, App, Context, ElementId, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	Window, div, prelude::FluentBuilder as _,
};
use protocol::{Snowflake, StatusKind};
use session::Command;
use store::Account;

use super::custom_status;
use crate::app::AppRoot;
use crate::components::{Avatar, Divider, IconButton, StatusDot};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::lookup::user_avatar;
use crate::model::Status;
use crate::shell::PluginStore;
use crate::theme::Theme;

const ACCOUNT_AVATAR_PIXELS: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
	Main,
	Accounts,
}

pub enum StatusMenuEvent {
	Close,
	EditProfile,
	LogOut,
}

pub struct StatusMenu {
	data: Entity<Data>,
	name: SharedString,
	handle: SharedString,
	avatar: String,
	status: Status,
	custom: Option<SharedString>,
	me: Option<Snowflake>,
	view: View,
	managing: bool,
	accounts: Vec<Account>,
}

impl EventEmitter<StatusMenuEvent> for StatusMenu {}

const CHOICES: [(StatusKind, Status, &str); 4] = [
	(StatusKind::Online, Status::Online, "status-online"),
	(StatusKind::Idle, Status::Idle, "status-idle"),
	(StatusKind::Dnd, Status::Dnd, "status-dnd"),
	(StatusKind::Invisible, Status::Offline, "status-invisible"),
];

impl StatusMenu {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let mut menu = Self {
			data,
			name: SharedString::default(),
			handle: SharedString::default(),
			avatar: String::new(),
			status: Status::Online,
			custom: None,
			me: None,
			view: View::Main,
			managing: false,
			accounts: Vec::new(),
		};
		menu.refresh(cx);
		menu
	}

	fn show_accounts(&mut self, cx: &mut Context<Self>) {
		self.reload_accounts(cx);
		self.managing = false;
		self.view = View::Accounts;
		cx.notify();
	}

	fn reload_accounts(&mut self, cx: &Context<Self>) {
		self.accounts = cx
			.try_global::<PluginStore>()
			.map(|store| store.0.accounts.list())
			.unwrap_or_default();
	}

	fn remove_account(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
		if let Some(store) = cx.try_global::<PluginStore>()
			&& let Err(error) = store.0.accounts.remove(user_id)
		{
			tracing::warn!(%error, "account was not removed");
		}
		self.reload_accounts(cx);
		cx.notify();
	}

	fn switch_to(&mut self, user_id: Snowflake, cx: &mut Context<Self>) {
		if self.me != Some(user_id) {
			self.data.read(cx).send(Command::SwitchAccount(user_id));
		}
		cx.emit(StatusMenuEvent::Close);
	}

	fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		let Some(user) = state.current_user() else {
			return;
		};
		let presence = state.presence(user.id);
		self.me = Some(user.id);
		self.name = state.display_name(None, user.id).into();
		self.handle = user.username.clone().into();
		self.avatar = user_avatar(user, 64);
		self.status = Status::from(&state.own_status());
		self.custom = presence
			.and_then(|presence| {
				presence
					.activities
					.iter()
					.find(|activity| activity.kind == protocol::ActivityKind::Custom)
			})
			.and_then(|activity| activity.state.clone())
			.filter(|text| !text.is_empty())
			.map(Into::into);
	}

	fn entry(
		&self,
		id: impl Into<ElementId>,
		leading: AnyElement,
		label: SharedString,
		danger: bool,
		theme: &Theme,
	) -> gpui::Stateful<gpui::Div> {
		div()
			.id(id)
			.flex()
			.items_center()
			.gap(theme.space_8)
			.h(theme.avatar_sm)
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.text_color(if danger {
				theme.danger
			} else {
				theme.interactive_normal
			})
			.hover(|style| {
				style.bg(theme.modifier_selected).text_color(if danger {
					theme.danger
				} else {
					theme.interactive_active
				})
			})
			.child(
				div()
					.flex()
					.flex_none()
					.justify_center()
					.w(theme.icon_md)
					.child(leading),
			)
			.child(div().flex_1().truncate().child(label))
	}
}

impl StatusMenu {
	fn accounts_view(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		let muted = |kind: IconKind| {
			Icon::new(kind)
				.size(theme.icon_md)
				.color(theme.interactive_normal)
				.into_any_element()
		};
		let back = self
			.entry(
				"status-accounts-back",
				muted(IconKind::ChevronLeft),
				cx.t("status-switch-accounts"),
				false,
				theme,
			)
			.on_click(cx.listener(|this, _, _, cx| {
				this.view = View::Main;
				this.managing = false;
				cx.notify();
			}));
		let rows: Vec<AnyElement> = self
			.accounts
			.iter()
			.enumerate()
			.map(|(index, account)| {
				let user_id = account.user_id;
				let current = Some(user_id) == self.me;
				let avatar = Avatar::new(account.username.clone())
					.size(theme.icon_md)
					.url(media::cdn::avatar_url(
						user_id.get(),
						account.avatar.as_deref(),
						None,
						ACCOUNT_AVATAR_PIXELS,
					))
					.into_any_element();
				let row = self.entry(
					("status-account", index),
					avatar,
					account.username.clone().into(),
					false,
					theme,
				);
				if self.managing {
					row.when(!current, |row| {
						row.child(
							IconButton::new(("status-account-remove", index), IconKind::Trash)
								.color(theme.danger)
								.tooltip(cx.t("accounts-remove"))
								.on_click(
									cx.listener(move |this, _, _, cx| this.remove_account(user_id, cx)),
								),
						)
					})
					.into_any_element()
				} else {
					row.when(current, |row| {
						row.child(
							Icon::new(IconKind::Check)
								.size(theme.icon_md)
								.color(theme.interactive_normal),
						)
					})
					.on_click(cx.listener(move |this, _, _, cx| this.switch_to(user_id, cx)))
					.into_any_element()
				}
			})
			.collect();
		let add = self
			.entry(
				"status-account-add",
				muted(IconKind::Plus),
				cx.t("accounts-add"),
				false,
				theme,
			)
			.on_click(cx.listener(|_, _, window, cx| {
				cx.emit(StatusMenuEvent::Close);
				AppRoot::add_account(window, cx);
			}));
		let (manage_label, manage_icon) = if self.managing {
			(cx.t("accounts-done"), IconKind::Check)
		} else {
			(cx.t("accounts-manage"), IconKind::UserCog)
		};
		let manage = self
			.entry(
				"status-account-manage",
				muted(manage_icon),
				manage_label,
				false,
				theme,
			)
			.on_click(cx.listener(|this, _, _, cx| {
				this.managing = !this.managing;
				cx.notify();
			}));
		let divider = || {
			div()
				.py(theme.space_4)
				.child(Divider::horizontal())
				.into_any_element()
		};
		let mut view = vec![back.into_any_element(), divider()];
		view.extend(rows);
		view.extend([divider(), add.into_any_element(), manage.into_any_element()]);
		view
	}
}

fn open_custom(data: Entity<Data>, window: &mut Window, cx: &mut App) {
	custom_status::open(data, window, cx);
}

impl Render for StatusMenu {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let current = self.status;
		let choices: Vec<AnyElement> = CHOICES
			.iter()
			.map(|(kind, status, key)| {
				let kind = kind.clone();
				let selected = *status == current;
				self.entry(
					*key,
					StatusDot::new(*status)
						.size(theme.icon_sm - theme.space_4)
						.cutout(theme.background_floating)
						.into_any_element(),
					cx.t(key),
					false,
					&theme,
				)
				.when(selected, |row| row.bg(theme.modifier_hover))
				.on_click(cx.listener(move |this, _, _, cx| {
					this.data.read(cx).send(Command::SetStatus(kind.clone()));
					cx.emit(StatusMenuEvent::Close);
				}))
				.into_any_element()
			})
			.collect();
		let custom_label = self.custom.clone().unwrap_or_else(|| cx.t("status-set-custom"));
		let data = self.data.clone();
		let edit = self
			.entry(
				"status-edit-profile",
				Icon::new(IconKind::Profile)
					.size(theme.icon_md)
					.color(theme.interactive_normal)
					.into_any_element(),
				cx.t("status-edit-profile"),
				false,
				&theme,
			)
			.on_click(cx.listener(|_, _, _, cx| cx.emit(StatusMenuEvent::EditProfile)));
		let custom = self
			.entry(
				"status-custom",
				Icon::new(IconKind::Emoji)
					.size(theme.icon_md)
					.color(theme.interactive_normal)
					.into_any_element(),
				custom_label,
				false,
				&theme,
			)
			.on_click(cx.listener(move |_, _, window, cx| {
				open_custom(data.clone(), window, cx);
				cx.emit(StatusMenuEvent::Close);
			}));
		let switch = self
			.entry(
				"status-switch",
				Icon::new(IconKind::Account)
					.size(theme.icon_md)
					.color(theme.interactive_normal)
					.into_any_element(),
				cx.t("status-switch-accounts"),
				false,
				&theme,
			)
			.on_click(cx.listener(|this, _, _, cx| this.show_accounts(cx)));
		let log_out = self
			.entry(
				"status-log-out",
				Icon::new(IconKind::LogOut)
					.size(theme.icon_md)
					.color(theme.danger)
					.into_any_element(),
				cx.t("settings-log-out"),
				true,
				&theme,
			)
			.on_click(cx.listener(|_, _, _, cx| cx.emit(StatusMenuEvent::LogOut)));
		let frame = div()
			.flex()
			.flex_col()
			.w(theme.status_popover_width)
			.p(theme.space_8)
			.gap(theme.space_2)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating);
		if self.view == View::Accounts {
			return frame.children(self.accounts_view(&theme, cx));
		}
		frame
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.p(theme.space_8)
					.child(
						Avatar::new(self.name.clone())
							.size(theme.avatar_md)
							.url(self.avatar.clone())
							.status(self.status)
							.cutout(theme.background_floating),
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
									.font_weight(FontWeight::BOLD)
									.text_color(theme.header_primary)
									.child(self.name.clone()),
							)
							.child(
								div()
									.truncate()
									.text_size(theme.font_xs)
									.text_color(theme.text_muted)
									.child(self.handle.clone()),
							),
					),
			)
			.child(div().py(theme.space_4).child(Divider::horizontal()))
			.child(edit)
			.child(div().py(theme.space_4).child(Divider::horizontal()))
			.children(choices)
			.child(div().py(theme.space_4).child(Divider::horizontal()))
			.child(custom)
			.child(div().py(theme.space_4).child(Divider::horizontal()))
			.child(switch)
			.child(log_out)
	}
}
