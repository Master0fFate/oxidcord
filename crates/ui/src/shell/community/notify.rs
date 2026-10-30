use std::rc::Rc;

use chrono::{DateTime, TimeDelta, Utc};
use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use protocol::{NotificationLevel, Snowflake};
use session::Command;

use super::data_handle;
use super::modal::{self, Close};
use crate::components::{ContextMenuBuilder, ToggleHandler, dialog_footer, switch_row};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::theme::{Theme, rem};

const MUTE_WINDOWS: [(&str, Option<i64>); 5] = [
	("mute-15-minutes", Some(15)),
	("mute-1-hour", Some(60)),
	("mute-8-hours", Some(480)),
	("mute-24-hours", Some(1440)),
	("mute-forever", None),
];

const LEVELS: [(&str, NotificationLevel); 3] = [
	("notify-all", NotificationLevel::AllMessages),
	("notify-mentions", NotificationLevel::OnlyMentions),
	("notify-nothing", NotificationLevel::NoMessages),
];

#[derive(Clone, Copy)]
pub enum Scope {
	Channel {
		guild: Option<Snowflake>,
		channel: Snowflake,
	},
	Guild(Snowflake),
	Thread {
		thread: Snowflake,
		post: bool,
	},
}

fn until(minutes: Option<i64>) -> Option<DateTime<Utc>> {
	minutes.map(|minutes| Utc::now() + TimeDelta::minutes(minutes))
}

pub fn current_level(data: &Data, scope: Scope) -> NotificationLevel {
	let state = data.state();
	match scope {
		Scope::Guild(guild) => state.notification_level(Some(guild), None),
		Scope::Thread { thread, .. } => state.notification_level(state.channel_guild(thread), Some(thread)),
		Scope::Channel { guild, channel } => state
			.guild_settings(guild)
			.and_then(|settings| {
				settings
					.channel_overrides
					.iter()
					.find(|entry| entry.channel_id == channel)
					.and_then(|entry| entry.message_notifications)
			})
			.unwrap_or(NotificationLevel::Inherit),
	}
}

pub fn mark_read_item(menu: ContextMenuBuilder, scope: Scope, unread: bool, cx: &App) -> ContextMenuBuilder {
	let label = cx.t("menu-mark-as-read");
	if !unread {
		return menu.disabled_item(label, Some(IconKind::Check));
	}
	menu.item(label, Some(IconKind::Check), move |_, cx| {
		let command = match scope {
			Scope::Channel { channel, .. } | Scope::Thread { thread: channel, .. } => {
				Command::MarkChannelRead { channel_id: channel }
			}
			Scope::Guild(guild) => Command::MarkGuildRead { guild_id: guild },
		};
		data_handle(cx).read(cx).send(command);
	})
}

pub fn mute_items(menu: ContextMenuBuilder, scope: Scope, muted: bool, cx: &App) -> ContextMenuBuilder {
	let noun = cx.t(match scope {
		Scope::Channel { .. } => "mute-channel",
		Scope::Guild(_) => "mute-server",
		Scope::Thread { post: true, .. } => "mute-post",
		Scope::Thread { post: false, .. } => "mute-thread",
	});
	if muted {
		let label = cx.t(match scope {
			Scope::Channel { .. } => "unmute-channel",
			Scope::Guild(_) => "unmute-server",
			Scope::Thread { post: true, .. } => "unmute-post",
			Scope::Thread { post: false, .. } => "unmute-thread",
		});
		return menu.item(label, Some(IconKind::Bell), move |_, cx| {
			let command = match scope {
				Scope::Channel { guild, channel } => Command::UnmuteChannel {
					channel_id: channel,
					guild_id: guild,
				},
				Scope::Guild(guild) => Command::UnmuteGuild { guild_id: guild },
				Scope::Thread { thread, .. } => Command::UnmuteThread { thread_id: thread },
			};
			data_handle(cx).read(cx).send(command);
		});
	}
	let durations = MUTE_WINDOWS
		.iter()
		.fold(ContextMenuBuilder::new(), |submenu, (key, minutes)| {
			let minutes = *minutes;
			submenu.item(cx.t(key), None, move |_, cx| {
				let end = until(minutes);
				let command = match scope {
					Scope::Channel { guild, channel } => Command::MuteChannel {
						channel_id: channel,
						guild_id: guild,
						until: end,
					},
					Scope::Guild(guild) => Command::MuteGuild {
						guild_id: guild,
						until: end,
					},
					Scope::Thread { thread, .. } => Command::MuteThread {
						thread_id: thread,
						until: end,
					},
				};
				data_handle(cx).read(cx).send(command);
			})
		});
	menu.submenu(noun, Some(IconKind::BellOff), durations)
}

pub fn level_items(menu: ContextMenuBuilder, scope: Scope, cx: &App) -> ContextMenuBuilder {
	let current = current_level(data_handle(cx).read(cx), scope);
	let mut levels = LEVELS
		.iter()
		.fold(ContextMenuBuilder::new(), |submenu, (key, level)| {
			let level = *level;
			let checked = current == level;
			submenu.checked_item(cx.t(key), checked, move |_, cx| {
				let data = data_handle(cx);
				match scope {
					Scope::Channel { channel, .. } => data.read(cx).send(Command::SetChannelNotifications {
						channel_id: channel,
						level,
					}),
					Scope::Guild(guild) => data.read(cx).send(Command::SetGuildNotifications {
						guild_id: Some(guild),
						level,
						suppress_everyone: false,
						suppress_roles: false,
						mobile_push: true,
					}),
					Scope::Thread { thread, .. } => data.read(cx).send(Command::SetThreadNotifications {
						thread_id: thread,
						level,
					}),
				}
			})
		});
	if matches!(scope, Scope::Channel { .. }) {
		levels = levels.checked_item(
			cx.t("notify-inherit"),
			current == NotificationLevel::Inherit,
			move |_, cx| {
				if let Scope::Channel { channel, .. } = scope {
					data_handle(cx).read(cx).send(Command::SetChannelNotifications {
						channel_id: channel,
						level: NotificationLevel::Inherit,
					});
				}
			},
		);
	}
	menu.submenu(cx.t("menu-notification-settings"), Some(IconKind::Bell), levels)
}

pub struct GuildNotifications {
	data: Entity<Data>,
	guild: Snowflake,
	level: NotificationLevel,
	suppress_everyone: bool,
	suppress_roles: bool,
	mobile_push: bool,
}

impl EventEmitter<Close> for GuildNotifications {}

pub fn open_guild_modal(window: &mut Window, cx: &mut App, guild: Snowflake) {
	let data = data_handle(cx);
	let (level, suppress_everyone, suppress_roles) = {
		let state = data.read(cx).state();
		let (suppress_everyone, suppress_roles) = state
			.guild_settings(Some(guild))
			.map_or((false, false), |settings| {
				(settings.suppress_everyone, settings.suppress_roles)
			});
		(
			state.notification_level(Some(guild), None),
			suppress_everyone,
			suppress_roles,
		)
	};
	let view = cx.new(|_| GuildNotifications {
		data,
		guild,
		level,
		suppress_everyone,
		suppress_roles,
		mobile_push: true,
	});
	let title = cx.t("notify-guild-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, None);
}

impl GuildNotifications {
	fn save(&self, cx: &mut Context<Self>) {
		self.data.read(cx).send(Command::SetGuildNotifications {
			guild_id: Some(self.guild),
			level: self.level,
			suppress_everyone: self.suppress_everyone,
			suppress_roles: self.suppress_roles,
			mobile_push: self.mobile_push,
		});
		cx.emit(Close);
	}

	fn toggle(&self, cx: &mut Context<Self>, apply: impl Fn(&mut Self, bool) + 'static) -> ToggleHandler {
		let weak = cx.weak_entity();
		Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				apply(this, value);
				cx.notify();
			})
			.ok();
		})
	}
}

impl Render for GuildNotifications {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let rows: Vec<_> = LEVELS
			.iter()
			.enumerate()
			.map(|(index, (key, level))| {
				let active = self.level == *level;
				let level = *level;
				div()
					.id(("notify-level", index))
					.flex()
					.items_center()
					.gap(theme.space_12)
					.h(theme.control_height + theme.space_8)
					.px(theme.space_12)
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.bg(if active {
						theme.modifier_selected
					} else {
						theme.background_secondary
					})
					.hover(|style| style.bg(theme.modifier_hover))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.level = level;
						cx.notify();
					}))
					.child(
						div()
							.flex()
							.items_center()
							.justify_center()
							.size(rem(20.))
							.rounded(theme.round(rem(20.)))
							.border_2()
							.border_color(if active {
								theme.brand
							} else {
								theme.interactive_muted
							})
							.when(active, |ring| {
								ring.child(div().size(rem(10.)).rounded_full().bg(theme.brand))
							}),
					)
					.child(
						div()
							.text_size(theme.font_md)
							.font_weight(FontWeight::MEDIUM)
							.text_color(theme.header_primary)
							.child(cx.t(key)),
					)
			})
			.collect();
		let weak_cancel = cx.weak_entity();
		let weak_save = cx.weak_entity();
		let everyone = self.toggle(cx, |this, value| this.suppress_everyone = value);
		let roles = self.toggle(cx, |this, value| this.suppress_roles = value);
		let push = self.toggle(cx, |this, value| this.mobile_push = value);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(div().flex().flex_col().gap(theme.space_4).children(rows))
			.child(switch_row(
				"notify-suppress-everyone",
				cx.t("notify-suppress-everyone"),
				None,
				self.suppress_everyone,
				&theme,
				everyone,
			))
			.child(switch_row(
				"notify-suppress-roles",
				cx.t("notify-suppress-roles"),
				None,
				self.suppress_roles,
				&theme,
				roles,
			))
			.child(switch_row(
				"notify-mobile-push",
				cx.t("notify-mobile-push"),
				None,
				self.mobile_push,
				&theme,
				push,
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("action-done"),
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
