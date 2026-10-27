use std::rc::Rc;

use gpui::{App, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use protocol::Snowflake;
use session::Command;

use super::edit;
use crate::components::ContextMenuBuilder;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::shell::community::notify::{self, Scope};
use crate::shell::community::{act, data_handle, developer_mode, raw};

type Toggle = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct ThreadFacts {
	pub thread: Snowflake,
	pub guild: Option<Snowflake>,
	pub name: String,
	pub post: bool,
	pub joined: bool,
	pub muted: bool,
	pub archived: bool,
	pub locked: bool,
	pub can_edit: bool,
	pub can_moderate: bool,
	pub full: bool,
}

fn send(command: Command, cx: &App) {
	data_handle(cx).read(cx).send(command);
}

fn copy(text: String, what: &'static str) {
	if let Err(error) = platform::clipboard::write_text(&text) {
		tracing::warn!(%error, what, "could not copy to the clipboard");
	}
}

fn membership(joined: bool, thread: Snowflake) -> Command {
	if joined {
		Command::LeaveThread { thread_id: thread }
	} else {
		Command::JoinThread { thread_id: thread }
	}
}

fn confirm_delete(window: &mut Window, cx: &mut App, thread: Snowflake, name: String, post: bool) {
	window.open_alert_dialog(cx, move |alert, _, cx| {
		alert
			.title(cx.t_args("thread-delete-title", &[("name", ArgValue::from(name.clone()))]))
			.description(cx.t(if post {
				"forum-delete-post-body"
			} else {
				"thread-delete-body"
			}))
			.confirm()
			.ok_text(cx.t(if post {
				"forum-delete-post"
			} else {
				"thread-delete"
			}))
			.ok_variant(ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				send(Command::DeleteThread { thread_id: thread }, cx);
				true
			})
	});
}

fn view_raw(window: &mut Window, cx: &mut App, thread: Snowflake) {
	let channel = data_handle(cx).read(cx).state().channel(thread).cloned();
	if let Some(channel) = channel {
		let title = cx.t("menu-view-raw");
		raw::open(window, cx, title, &channel);
	}
}

pub fn thread_menu(
	facts: ThreadFacts,
	toggle_full: impl Fn(&mut Window, &mut App) + 'static,
) -> ContextMenuBuilder {
	let toggle: Toggle = Rc::new(toggle_full);
	ContextMenuBuilder::new().lazy(move |cx| build(&facts, &toggle, cx))
}

fn build(facts: &ThreadFacts, toggle: &Toggle, cx: &App) -> ContextMenuBuilder {
	let thread = facts.thread;
	let post = facts.post;
	let joined = facts.joined;
	let mut menu = ContextMenuBuilder::new();
	if post {
		menu = menu.item(
			cx.t(if joined {
				"forum-unfollow-post"
			} else {
				"forum-follow-post"
			}),
			Some(if joined { IconKind::BellOff } else { IconKind::Bell }),
			move |_, cx| send(membership(joined, thread), cx),
		);
	}
	let toggle = toggle.clone();
	menu = menu
		.item(
			cx.t(if facts.full {
				"thread-close-full-view"
			} else {
				"thread-open-full-view"
			}),
			Some(if facts.full {
				IconKind::Shrink
			} else {
				IconKind::Expand
			}),
			move |window, cx| toggle(window, cx),
		)
		.separator();
	menu = notification_items(menu, facts, cx);
	if facts.can_edit {
		let name = if post { "forum-edit-post" } else { "thread-edit" };
		menu = menu
			.separator()
			.item(cx.t(name), Some(IconKind::Edit), move |window, cx| {
				edit::open(window, cx, data_handle(cx), thread, post);
			});
	}
	menu = menu
		.separator()
		.external_item(
			cx.t("thread-search"),
			Some(IconKind::Search),
			move |window, cx| {
				act(window, cx, |community, _, cx| community.search_in(thread, cx));
			},
		)
		.external_item(cx.t("thread-pins"), Some(IconKind::Pin), |window, cx| {
			let at = window.mouse_position();
			act(window, cx, |community, window, cx| {
				community.open_pins(at, window, cx)
			});
		});
	menu = moderation_items(menu, facts, cx);
	if !post {
		menu = menu.separator().item(
			cx.t(if joined { "thread-leave" } else { "thread-join" }),
			Some(if joined { IconKind::LogOut } else { IconKind::Join }),
			move |_, cx| send(membership(joined, thread), cx),
		);
	}
	tail_items(menu.separator(), facts, cx)
}

fn notification_items(menu: ContextMenuBuilder, facts: &ThreadFacts, cx: &App) -> ContextMenuBuilder {
	let scope = Scope::Thread {
		thread: facts.thread,
		post: facts.post,
	};
	if facts.joined {
		return notify::level_items(notify::mute_items(menu, scope, facts.muted, cx), scope, cx);
	}
	menu.disabled_item(
		cx.t(if facts.post { "mute-post" } else { "mute-thread" }),
		Some(IconKind::BellOff),
	)
	.disabled_item(cx.t("menu-notification-settings"), Some(IconKind::Bell))
}

fn moderation_items(menu: ContextMenuBuilder, facts: &ThreadFacts, cx: &App) -> ContextMenuBuilder {
	if !facts.can_edit && !facts.can_moderate {
		return menu;
	}
	let thread = facts.thread;
	let post = facts.post;
	let mut menu = menu.separator();
	if facts.can_edit {
		let archived = facts.archived;
		let label = match (archived, post) {
			(false, false) => "thread-close",
			(false, true) => "forum-close-post",
			(true, false) => "thread-reopen",
			(true, true) => "forum-reopen-post",
		};
		menu = menu.item(
			cx.t(label),
			Some(if archived {
				IconKind::Unarchive
			} else {
				IconKind::Archive
			}),
			move |_, cx| {
				send(
					Command::ArchiveThread {
						thread_id: thread,
						archived: !archived,
					},
					cx,
				)
			},
		);
	}
	if facts.can_moderate {
		let locked = facts.locked;
		let lock_label = match (locked, post) {
			(false, false) => "thread-lock",
			(false, true) => "forum-lock-post",
			(true, false) => "thread-unlock",
			(true, true) => "forum-unlock-post",
		};
		let name = facts.name.clone();
		menu = menu
			.item(cx.t(lock_label), Some(IconKind::Lock), move |_, cx| {
				send(
					Command::LockThread {
						thread_id: thread,
						locked: !locked,
					},
					cx,
				)
			})
			.danger_item(
				cx.t(if post {
					"forum-delete-post"
				} else {
					"thread-delete"
				}),
				Some(IconKind::Trash),
				move |window, cx| confirm_delete(window, cx, thread, name.clone(), post),
			);
	}
	menu
}

fn tail_items(menu: ContextMenuBuilder, facts: &ThreadFacts, cx: &App) -> ContextMenuBuilder {
	let thread = facts.thread;
	let scope = facts
		.guild
		.map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
	let menu = menu
		.item(cx.t("menu-copy-link"), Some(IconKind::Link), move |_, _| {
			copy(
				format!("https://discord.com/channels/{scope}/{thread}"),
				"thread link",
			);
		})
		.item(cx.t("menu-view-raw"), Some(IconKind::Code), move |window, cx| {
			view_raw(window, cx, thread)
		});
	if !developer_mode(cx) {
		return menu;
	}
	let label = if facts.post {
		"menu-copy-post-id"
	} else {
		"menu-copy-thread-id"
	};
	menu.item(cx.t(label), Some(IconKind::Id), move |_, _| {
		copy(thread.to_string(), "thread id");
	})
}
