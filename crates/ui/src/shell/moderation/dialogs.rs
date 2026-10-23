use std::rc::Rc;

use chrono::{TimeDelta, Utc};
use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::Snowflake;
use session::Command;

use crate::components::{PickHandler, dialog_footer, field, hint, segmented, select};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::shell::community::data_handle;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const NICK_LIMIT: usize = 32;
const REASON_LIMIT: usize = 512;
const TIMEOUTS: [(&str, i64); 6] = [
	("mod-timeout-60s", 60),
	("mod-timeout-5m", 300),
	("mod-timeout-10m", 600),
	("mod-timeout-1h", 3600),
	("mod-timeout-1d", 86_400),
	("mod-timeout-1w", 604_800),
];
const HISTORY: [(&str, u32); 7] = [
	("mod-history-none", 0),
	("mod-history-1h", 3600),
	("mod-history-6h", 21_600),
	("mod-history-12h", 43_200),
	("mod-history-24h", 86_400),
	("mod-history-3d", 259_200),
	("mod-history-7d", 604_800),
];

pub struct Nickname {
	data: Entity<Data>,
	guild: Snowflake,
	user: Snowflake,
	input: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for Nickname {}

pub fn open_nickname(window: &mut Window, cx: &mut App, guild: Snowflake, user: Snowflake, current: String) {
	let data = data_handle(cx);
	let placeholder = cx.t("mod-nickname-placeholder");
	let view = cx.new(|cx| {
		let input = cx.new(|cx| {
			let mut state = InputState::new(window, cx).placeholder(placeholder);
			state.set_value(current, window, cx);
			state
		});
		let subscription = cx.subscribe_in(
			&input,
			window,
			|this: &mut Nickname, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::PressEnter { .. }) {
					this.save(cx);
				}
			},
		);
		Nickname {
			data,
			guild,
			user,
			input,
			_subscription: subscription,
		}
	});
	let focus = view.read(cx).input.read(cx).focus_handle(cx);
	let title = cx.t("mod-change-nickname");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl Nickname {
	fn save(&mut self, cx: &mut Context<Self>) {
		let value: String = self
			.input
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(NICK_LIMIT)
			.collect();
		self.data.read(cx).send(Command::SetNickname {
			guild_id: self.guild,
			user_id: self.user,
			nick: (!value.is_empty()).then_some(value),
		});
		cx.emit(Close);
	}
}

impl Render for Nickname {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let weak_cancel = cx.weak_entity();
		let weak_save = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("mod-nickname"), &theme, Input::new(&self.input)))
			.child(hint(cx.t("mod-nickname-hint"), &theme))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("settings-save"),
				false,
				true,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_save.update(cx, |this, cx| this.save(cx)).ok();
				},
			))
	}
}

pub struct Timeout {
	data: Entity<Data>,
	guild: Snowflake,
	user: Snowflake,
	name: SharedString,
	choice: usize,
}

impl EventEmitter<Close> for Timeout {}

pub fn open_timeout(window: &mut Window, cx: &mut App, guild: Snowflake, user: Snowflake, name: String) {
	let data = data_handle(cx);
	let view = cx.new(|_| Timeout {
		data,
		guild,
		user,
		name: name.clone().into(),
		choice: 2,
	});
	let title = cx.t_args("mod-timeout-title", &[("name", ArgValue::from(name))]);
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, None);
}

impl Timeout {
	fn confirm(&mut self, cx: &mut Context<Self>) {
		let seconds = TIMEOUTS[self.choice].1;
		self.data.read(cx).send(Command::TimeoutMember {
			guild_id: self.guild,
			user_id: self.user,
			until: Some(Utc::now() + TimeDelta::seconds(seconds)),
		});
		cx.emit(Close);
	}
}

impl Render for Timeout {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels: Vec<SharedString> = TIMEOUTS.iter().map(|(key, _)| cx.t(key)).collect();
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.choice = index;
				cx.notify();
			})
			.ok();
		});
		let weak_cancel = cx.weak_entity();
		let weak_confirm = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(hint(
				cx.t_args(
					"mod-timeout-body",
					&[("name", ArgValue::from(self.name.to_string()))],
				),
				&theme,
			))
			.child(field(
				cx.t("mod-timeout-duration"),
				&theme,
				segmented("mod-timeout-choice", &labels, self.choice, &theme, pick),
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("mod-timeout"),
				false,
				true,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_confirm.update(cx, |this, cx| this.confirm(cx)).ok();
				},
			))
	}
}

pub struct Kick {
	data: Entity<Data>,
	guild: Snowflake,
	user: Snowflake,
	name: SharedString,
	reason: Entity<InputState>,
}

impl EventEmitter<Close> for Kick {}

pub fn open_kick(window: &mut Window, cx: &mut App, guild: Snowflake, user: Snowflake, name: String) {
	let data = data_handle(cx);
	let placeholder = cx.t("mod-reason-placeholder");
	let view = cx.new(|cx| Kick {
		data,
		guild,
		user,
		name: name.clone().into(),
		reason: cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)),
	});
	let focus = view.read(cx).reason.read(cx).focus_handle(cx);
	let title = cx.t_args("mod-kick-title", &[("name", ArgValue::from(name))]);
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl Kick {
	fn confirm(&mut self, cx: &mut Context<Self>) {
		let reason: String = self
			.reason
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(REASON_LIMIT)
			.collect();
		self.data.read(cx).send(Command::KickMember {
			guild_id: self.guild,
			user_id: self.user,
			reason: (!reason.is_empty()).then_some(reason),
		});
		cx.emit(Close);
	}
}

impl Render for Kick {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let weak_cancel = cx.weak_entity();
		let weak_confirm = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(hint(
				cx.t_args(
					"mod-kick-body",
					&[("name", ArgValue::from(self.name.to_string()))],
				),
				&theme,
			))
			.child(field(cx.t("mod-reason"), &theme, Input::new(&self.reason)))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("mod-kick"),
				true,
				true,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_confirm.update(cx, |this, cx| this.confirm(cx)).ok();
				},
			))
	}
}

pub struct Ban {
	data: Entity<Data>,
	guild: Snowflake,
	user: Snowflake,
	name: SharedString,
	history: usize,
	reason: Entity<InputState>,
}

impl EventEmitter<Close> for Ban {}

pub fn open_ban(window: &mut Window, cx: &mut App, guild: Snowflake, user: Snowflake, name: String) {
	let data = data_handle(cx);
	let placeholder = cx.t("mod-reason-placeholder");
	let view = cx.new(|cx| Ban {
		data,
		guild,
		user,
		name: name.clone().into(),
		history: 0,
		reason: cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)),
	});
	let focus = view.read(cx).reason.read(cx).focus_handle(cx);
	let title = cx.t_args("mod-ban-title", &[("name", ArgValue::from(name))]);
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl Ban {
	fn confirm(&mut self, cx: &mut Context<Self>) {
		let reason: String = self
			.reason
			.read(cx)
			.value()
			.trim()
			.chars()
			.take(REASON_LIMIT)
			.collect();
		self.data.read(cx).send(Command::BanMember {
			guild_id: self.guild,
			user_id: self.user,
			delete_message_seconds: HISTORY[self.history].1,
			reason: (!reason.is_empty()).then_some(reason),
		});
		cx.emit(Close);
	}
}

impl Render for Ban {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels: Vec<SharedString> = HISTORY.iter().map(|(key, _)| cx.t(key)).collect();
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.history = index;
				cx.notify();
			})
			.ok();
		});
		let weak_cancel = cx.weak_entity();
		let weak_confirm = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(hint(
				cx.t_args("mod-ban-body", &[("name", ArgValue::from(self.name.to_string()))]),
				&theme,
			))
			.child(field(
				cx.t("mod-ban-history"),
				&theme,
				select(
					"mod-ban-history",
					labels[self.history].clone(),
					&labels,
					self.history,
					pick,
				),
			))
			.child(field(cx.t("mod-reason"), &theme, Input::new(&self.reason)))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("mod-ban"),
				true,
				true,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_confirm.update(cx, |this, cx| this.confirm(cx)).ok();
				},
			))
	}
}

pub fn open_bulk_delete(window: &mut Window, cx: &mut App, channel: Snowflake, ids: Vec<Snowflake>) {
	let data = data_handle(cx);
	let count = ids.len();
	window.open_alert_dialog(cx, move |alert, _, cx| {
		let data = data.clone();
		let ids = ids.clone();
		alert
			.title(cx.t_args("mod-bulk-title", &[("count", ArgValue::from(count))]))
			.description(cx.t("mod-bulk-body"))
			.confirm()
			.ok_text(cx.t("menu-delete"))
			.ok_variant(ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				data.read(cx).send(Command::DeleteMessagesBulk {
					channel_id: channel,
					ids: ids.clone(),
				});
				true
			})
	});
}
