use gpui::{App, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use protocol::Snowflake;
use session::Command;

use super::profile_data::{Relation, RelationFacts};
use crate::components::ContextMenuBuilder;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::shell::community::data_handle;
use crate::shell::moderation::dialogs::open_friend_nickname;

#[derive(Debug, Clone, Copy)]
enum Destructive {
	Remove,
	Block,
}

impl Destructive {
	fn keys(self) -> (&'static str, &'static str, &'static str) {
		match self {
			Self::Remove => ("relation-remove-title", "relation-remove-body", "friends-remove"),
			Self::Block => ("relation-block-title", "relation-block-body", "friends-block"),
		}
	}

	fn command(self, user_id: Snowflake) -> Command {
		match self {
			Self::Remove => Command::RemoveRelationship { user_id },
			Self::Block => Command::BlockUser { user_id },
		}
	}
}

fn send(cx: &App, command: Command) {
	data_handle(cx).read(cx).send(command);
}

fn confirm(window: &mut Window, cx: &mut App, user: Snowflake, name: SharedString, action: Destructive) {
	let (title, body, ok) = action.keys();
	window.open_alert_dialog(cx, move |alert, _, cx| {
		alert
			.title(cx.t(title))
			.description(cx.t_args(body, &[("name", ArgValue::from(name.to_string()))]))
			.confirm()
			.ok_text(cx.t(ok))
			.ok_variant(ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				send(cx, action.command(user));
				true
			})
	});
}

pub struct PrimaryAction {
	pub label: &'static str,
	pub icon: IconKind,
	pub command: fn(Snowflake) -> Command,
}

pub fn primary_action(relation: Relation) -> Option<PrimaryAction> {
	let (label, icon, command): (_, _, fn(Snowflake) -> Command) = match relation {
		Relation::None => ("profile-add-friend", IconKind::FriendRequests, |user_id| {
			Command::AcceptFriend { user_id }
		}),
		Relation::Incoming => ("friends-accept", IconKind::Check, |user_id| {
			Command::AcceptFriend { user_id }
		}),
		Relation::Outgoing => ("menu-cancel-request", IconKind::Close, |user_id| {
			Command::RemoveRelationship { user_id }
		}),
		Relation::Blocked => ("friends-unblock", IconKind::Shield, |user_id| {
			Command::RemoveRelationship { user_id }
		}),
		Relation::Friend => return None,
	};
	Some(PrimaryAction { label, icon, command })
}

pub fn primary_items(
	menu: ContextMenuBuilder,
	user: Snowflake,
	relation: Relation,
	cx: &App,
) -> ContextMenuBuilder {
	match primary_action(relation) {
		Some(action) => menu.item(cx.t(action.label), Some(action.icon), move |_, cx| {
			send(cx, (action.command)(user));
		}),
		None => menu,
	}
}

pub fn manage_items(
	menu: ContextMenuBuilder,
	user: Snowflake,
	name: &SharedString,
	facts: &RelationFacts,
	cx: &App,
) -> ContextMenuBuilder {
	let menu = if facts.relation == Relation::Friend {
		let nickname = facts.nickname.clone().unwrap_or_default();
		let remove_name = name.clone();
		menu.item(
			cx.t("menu-edit-friend-nickname"),
			Some(IconKind::Edit),
			move |window, cx| open_friend_nickname(window, cx, user, nickname.clone()),
		)
		.danger_item(
			cx.t("friends-remove"),
			Some(IconKind::Trash),
			move |window, cx| {
				confirm(window, cx, user, remove_name.clone(), Destructive::Remove);
			},
		)
	} else {
		menu
	};
	if facts.relation == Relation::Blocked {
		return menu;
	}
	let menu = if facts.ignored {
		menu.item(cx.t("menu-unignore"), Some(IconKind::Eye), move |_, cx| {
			send(cx, Command::UnignoreUser { user_id: user })
		})
	} else {
		menu.item(cx.t("menu-ignore"), Some(IconKind::EyeOff), move |_, cx| {
			send(cx, Command::IgnoreUser { user_id: user })
		})
	};
	let block_name = name.clone();
	menu.danger_item(
		cx.t("friends-block"),
		Some(IconKind::Shield),
		move |window, cx| {
			confirm(window, cx, user, block_name.clone(), Destructive::Block);
		},
	)
}
