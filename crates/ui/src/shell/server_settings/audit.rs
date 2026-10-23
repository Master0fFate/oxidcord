use std::rc::Rc;

use gpui::{
	AnyElement, App, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render, SharedString,
	Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::button::Button;
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use serde_json::Value;
use session::Command;
use state::{AuditEntry, State};

use super::kit;
use crate::components::{Avatar, PickHandler, select};
use crate::data::{Data, DataEvent};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::lookup::user_avatar;
use crate::theme::Theme;

const FILTERS: [i64; 22] = [
	-1, 1, 10, 11, 12, 20, 22, 23, 24, 25, 30, 31, 32, 40, 42, 60, 61, 62, 72, 74, 75, 110,
];
const VALUE_LIMIT: usize = 60;
const CHANGE_LIMIT: usize = 5;

pub struct AuditPage {
	data: Entity<Data>,
	guild: Snowflake,
	filter: usize,
	_subscription: Subscription,
}

fn known_action(action_type: i64) -> bool {
	matches!(
		action_type,
		1 | 10
			| 11 | 12 | 13
			| 14 | 15 | 20
			| 21 | 22 | 23
			| 24 | 25 | 26
			| 27 | 30 | 31
			| 32 | 40 | 41
			| 42 | 50 | 51
			| 52 | 60 | 61
			| 62 | 72 | 73
			| 74 | 75 | 110
			| 111 | 112
	)
}

fn show(value: &Value) -> String {
	let text = match value {
		Value::String(text) => text.clone(),
		Value::Null => String::new(),
		Value::Array(items) => format!("[{}]", items.len()),
		other => other.to_string(),
	};
	if text.chars().count() > VALUE_LIMIT {
		let mut shortened: String = text.chars().take(VALUE_LIMIT).collect();
		shortened.push('…');
		shortened
	} else {
		text
	}
}

fn target_label(state: &State, guild: Snowflake, entry: &AuditEntry) -> String {
	let named = entry.changes.iter().find_map(|change| {
		matches!(change.key.as_str(), "name" | "code")
			.then(|| change.new.as_ref().or(change.old.as_ref()).map(show))
			.flatten()
	});
	let Some(id) = entry.target_id else {
		return named.unwrap_or_default();
	};
	match entry.action_type {
		20 | 22 | 23 | 24 | 25 | 26 | 27 | 72 | 74 | 75 => state.display_name(Some(guild), id),
		10..=15 | 110..=112 => state
			.channel(id)
			.and_then(|channel| channel.name.clone())
			.or(named)
			.map_or_else(|| id.to_string(), |name| format!("#{name}")),
		30..=32 => state
			.guild(guild)
			.and_then(|found| found.roles.get(&id).map(|role| role.name.clone()))
			.or(named)
			.unwrap_or_else(|| id.to_string()),
		_ => named.unwrap_or_else(|| id.to_string()),
	}
}

impl AuditPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, cx: &mut Context<Self>) -> Self {
		data.read(cx).send(Command::LoadAuditLog {
			guild_id: guild,
			before: None,
			action_type: None,
		});
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			if matches!(event, DataEvent::Changed(_) | DataEvent::MediaReady) {
				cx.notify();
			}
		});
		Self {
			data,
			guild,
			filter: 0,
			_subscription: subscription,
		}
	}

	fn action_type(&self) -> Option<i64> {
		Some(FILTERS[self.filter]).filter(|value| *value >= 0)
	}

	fn reload(&self, cx: &App) {
		self.data.read(cx).send(Command::LoadAuditLog {
			guild_id: self.guild,
			before: None,
			action_type: self.action_type(),
		});
	}

	fn load_more(&self, cx: &App) {
		let before = self
			.data
			.read(cx)
			.state()
			.audit_log(self.guild)
			.and_then(|log| log.entries.last().map(|entry| entry.id));
		self.data.read(cx).send(Command::LoadAuditLog {
			guild_id: self.guild,
			before,
			action_type: self.action_type(),
		});
	}

	fn row(&self, entry: &AuditEntry, state: &State, theme: &Theme, cx: &App) -> AnyElement {
		let actor = entry
			.user_id
			.map(|id| state.display_name(Some(self.guild), id))
			.unwrap_or_default();
		let target = target_label(state, self.guild, entry);
		let sentence = if known_action(entry.action_type) {
			cx.t_args(
				&format!("audit-action-{}", entry.action_type),
				&[
					("actor", ArgValue::from(actor.clone())),
					("target", ArgValue::from(target)),
				],
			)
		} else {
			cx.t_args(
				"audit-action-unknown",
				&[("actor", ArgValue::from(actor.clone()))],
			)
		};
		let avatar = entry
			.user
			.as_ref()
			.map(|user| user_avatar(user, 64))
			.unwrap_or_default();
		let changes: Vec<AnyElement> = entry
			.changes
			.iter()
			.take(CHANGE_LIMIT)
			.map(|change| {
				let old = change.old.as_ref().map(show).unwrap_or_default();
				let new = change.new.as_ref().map(show).unwrap_or_default();
				div()
					.flex()
					.gap(theme.space_8)
					.text_size(theme.font_sm)
					.child(
						div()
							.flex_none()
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_secondary)
							.child(change.key.clone()),
					)
					.when(!old.is_empty(), |line| {
						line.child(
							div()
								.min_w_0()
								.truncate()
								.text_color(theme.danger)
								.line_through()
								.child(old.clone()),
						)
					})
					.when(!new.is_empty(), |line| {
						line.child(
							div()
								.min_w_0()
								.truncate()
								.text_color(theme.success)
								.child(new.clone()),
						)
					})
					.into_any_element()
			})
			.collect();
		let when: SharedString = format::message_time(entry.id.timestamp(), cx);
		div()
			.flex()
			.gap(theme.space_12)
			.p(theme.space_12)
			.border_b_1()
			.border_color(theme.divider)
			.child(Avatar::new(actor.clone()).size(theme.portrait_md).url(avatar))
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap(theme.space_4)
					.child(
						div()
							.flex()
							.items_baseline()
							.justify_between()
							.gap(theme.space_8)
							.child(
								div()
									.text_size(theme.font_md)
									.text_color(theme.text_normal)
									.child(sentence),
							)
							.child(
								div()
									.flex_none()
									.text_size(theme.font_tiny)
									.text_color(theme.text_muted)
									.child(when),
							),
					)
					.when(!changes.is_empty(), |column| {
						column.child(
							div()
								.flex()
								.flex_col()
								.gap(theme.space_2)
								.pl(theme.space_8)
								.border_l_2()
								.border_color(theme.divider)
								.children(changes),
						)
					})
					.children(entry.reason.as_ref().map(|reason| {
						div()
							.text_size(theme.font_sm)
							.italic()
							.text_color(theme.text_muted)
							.child(cx.t_args("audit-reason", &[("reason", ArgValue::from(reason.clone()))]))
					})),
			)
			.into_any_element()
	}
}

impl Render for AuditPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let labels: Vec<SharedString> = FILTERS
			.iter()
			.map(|value| {
				if *value < 0 {
					cx.t("audit-filter-all")
				} else {
					cx.t(&format!("audit-filter-{value}"))
				}
			})
			.collect();
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, _, cx| {
			weak.update(cx, |this, cx| {
				this.filter = index;
				this.reload(cx);
				cx.notify();
			})
			.ok();
		});
		let (rows, has_more, loaded) = {
			let data = self.data.read(cx);
			let state = data.state();
			match state.audit_log(self.guild) {
				Some(log) => (
					log.entries
						.iter()
						.map(|entry| self.row(entry, &state, &theme, cx))
						.collect::<Vec<_>>(),
					log.has_more,
					true,
				),
				None => (Vec::new(), false, false),
			}
		};
		let empty = loaded && rows.is_empty();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-audit-log"), &theme))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_12)
					.pb(theme.space_16)
					.child(select(
						"audit-filter",
						labels[self.filter].clone(),
						&labels,
						self.filter,
						pick,
					)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.pb(theme.space_32)
					.children(rows)
					.when(empty, |list| {
						list.child(kit::empty(IconKind::AuditLog, cx.t("audit-empty"), &theme))
					})
					.when(has_more, |list| {
						list.child(
							div().flex().justify_center().pt(theme.space_16).child(
								Button::new("audit-more")
									.outline()
									.label(cx.t("audit-load-more"))
									.on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
							),
						)
					})
					.overflow_y_scrollbar()
					.id("audit-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
