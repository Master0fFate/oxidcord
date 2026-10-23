use gpui::{App, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariant;
use protocol::{ChannelType, Permissions, Snowflake};
use session::Command;

use super::access::GuildAccess;
use super::notify::{self, Scope};
use super::{act, data_handle, developer_mode};
use crate::components::ContextMenuBuilder;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::shell::server_settings::channel;

pub struct ChannelFacts {
	pub id: Snowflake,
	pub kind: ChannelType,
	pub name: String,
	pub unread: bool,
	pub muted: bool,
	pub joined: bool,
}

fn copy(text: String, what: &'static str) {
	if let Err(error) = platform::clipboard::write_text(&text) {
		tracing::warn!(%error, what, "could not copy to the clipboard");
	}
}

fn is_thread(kind: ChannelType) -> bool {
	matches!(
		kind,
		ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::AnnouncementThread
	)
}

pub fn channel_menu(
	guild: Option<Snowflake>,
	facts: ChannelFacts,
	access: GuildAccess,
) -> ContextMenuBuilder {
	ContextMenuBuilder::new().lazy(move |cx| channel_menu_now(guild, &facts, &access, cx))
}

fn channel_menu_now(
	guild: Option<Snowflake>,
	facts: &ChannelFacts,
	access: &GuildAccess,
	cx: &App,
) -> ContextMenuBuilder {
	let id = facts.id;
	let scope = Scope::Channel { guild, channel: id };
	let mut menu = notify::mark_read_item(ContextMenuBuilder::new(), scope, facts.unread, cx).separator();
	let thread = is_thread(facts.kind);
	if guild.is_some()
		&& !thread
		&& facts.kind != ChannelType::GuildCategory
		&& access.has(Permissions::CREATE_INSTANT_INVITE)
	{
		menu = menu.item(
			cx.t("menu-invite-channel"),
			Some(IconKind::Members),
			move |window, cx| {
				crate::shell::server_settings::invites::open_create(window, cx, id);
			},
		);
	}
	menu = notify::level_items(notify::mute_items(menu, scope, facts.muted, cx), scope, cx);
	if thread {
		let joined = facts.joined;
		menu = menu.separator().item(
			cx.t(if joined { "thread-leave" } else { "thread-join" }),
			Some(if joined { IconKind::LogOut } else { IconKind::Join }),
			move |_, cx| {
				let command = if joined {
					Command::LeaveThread { thread_id: id }
				} else {
					Command::JoinThread { thread_id: id }
				};
				data_handle(cx).read(cx).send(command);
			},
		);
	}
	if let Some(guild) = guild
		&& !thread
		&& access.has(Permissions::MANAGE_CHANNELS)
	{
		let name = facts.name.clone();
		menu = menu
			.separator()
			.item(
				cx.t("menu-edit-channel"),
				Some(IconKind::Settings),
				move |window, cx| {
					channel::open_edit(window, cx, guild, id, false);
				},
			)
			.danger_item(
				cx.t("menu-delete-channel"),
				Some(IconKind::Trash),
				move |window, cx| {
					channel::confirm_delete(window, cx, id, name.clone());
				},
			);
	}
	let link_guild = guild;
	let menu = menu
		.separator()
		.item(cx.t("menu-copy-link"), Some(IconKind::Link), move |_, _| {
			let scope = link_guild.map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
			copy(
				format!("https://discord.com/channels/{scope}/{id}"),
				"channel link",
			);
		});
	if developer_mode(cx) {
		menu.item(cx.t("menu-copy-id"), Some(IconKind::Copy), move |_, _| {
			copy(id.to_string(), "channel id");
		})
	} else {
		menu
	}
}

fn confirm_leave(window: &mut Window, cx: &mut App, guild: Snowflake, name: String) {
	let data = data_handle(cx);
	window.open_alert_dialog(cx, move |alert, _, cx| {
		let data = data.clone();
		alert
			.title(cx.t_args("leave-server-title", &[("name", ArgValue::from(name.clone()))]))
			.description(cx.t("leave-server-body"))
			.confirm()
			.ok_text(cx.t("menu-leave-server"))
			.ok_variant(ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				data.read(cx).send(Command::LeaveGuild { guild_id: guild });
				true
			})
	});
}

fn guild_tail(guild: Snowflake, cx: &App) -> ContextMenuBuilder {
	let (name, owner) = {
		let data = data_handle(cx);
		let data = data.read(cx);
		let state = data.state();
		let found = state.guild(guild);
		(
			found.map(|found| found.name.clone()).unwrap_or_default(),
			found.is_some_and(|found| found.owner_id.is_some() && found.owner_id == state.current_user_id()),
		)
	};
	let mut menu = ContextMenuBuilder::new();
	if developer_mode(cx) {
		menu = menu.item(cx.t("menu-copy-id"), Some(IconKind::Copy), move |_, _| {
			copy(guild.to_string(), "server id");
		});
	}
	if !owner {
		menu = menu.danger_item(
			cx.t("menu-leave-server"),
			Some(IconKind::LogOut),
			move |window, cx| confirm_leave(window, cx, guild, name.clone()),
		);
	}
	if menu.is_empty() {
		menu
	} else {
		ContextMenuBuilder::new().separator().extend(menu)
	}
}

pub fn guild_menu(
	guild: Snowflake,
	invite_channel: Option<Snowflake>,
	unread: bool,
	muted: bool,
	access: &GuildAccess,
	cx: &App,
) -> ContextMenuBuilder {
	let mut menu = ContextMenuBuilder::new();
	if let Some(channel) = invite_channel
		&& access.has(Permissions::CREATE_INSTANT_INVITE)
	{
		menu = menu.item(
			cx.t("menu-invite-people"),
			Some(IconKind::Members),
			move |window, cx| {
				crate::shell::server_settings::invites::open_create(window, cx, channel);
			},
		);
	}
	if access.can_open_settings() {
		menu = menu.item(
			cx.t("menu-server-settings"),
			Some(IconKind::Settings),
			move |window, cx| {
				act(window, cx, move |community, window, cx| {
					community.open_server_settings(guild, None, window, cx)
				});
			},
		);
	}
	if access.has(Permissions::MANAGE_CHANNELS) {
		menu = menu.item(
			cx.t("menu-create-channel"),
			Some(IconKind::Plus),
			move |window, cx| {
				channel::open_create(window, cx, guild, None);
			},
		);
	}
	let scope = Scope::Guild(guild);
	let menu = if menu.is_empty() { menu } else { menu.separator() };
	let menu = notify::mark_read_item(menu, scope, unread, cx);
	let menu = notify::mute_items(menu, scope, muted, cx);
	menu.item(
		cx.t("menu-notification-settings"),
		Some(IconKind::Bell),
		move |window, cx| notify::open_guild_modal(window, cx, guild),
	)
	.lazy(move |cx| guild_tail(guild, cx))
}

pub fn guild_menu_for(guild: Snowflake) -> ContextMenuBuilder {
	ContextMenuBuilder::new().lazy(move |cx| guild_menu_now(guild, cx))
}

fn guild_menu_now(guild: Snowflake, cx: &App) -> ContextMenuBuilder {
	let data = data_handle(cx);
	let (access, invite_channel, unread, muted) = {
		let state = data.read(cx).state();
		let invite_channel = state
			.guild_channel_tree(guild)
			.iter()
			.flat_map(|node| node.children.iter())
			.find(|row| !row.locked && row.kind == ChannelType::GuildText)
			.map(|row| row.id);
		let summary = state.guild_summary(guild);
		(
			GuildAccess::of(&state, guild),
			invite_channel,
			summary
				.as_ref()
				.is_some_and(|summary| summary.unread || summary.mentions > 0),
			summary.is_some_and(|summary| summary.muted),
		)
	};
	guild_menu(guild, invite_channel, unread, muted, &access, cx)
}
