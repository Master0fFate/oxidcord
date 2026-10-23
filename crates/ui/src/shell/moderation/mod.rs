pub mod dialogs;

use chrono::Utc;
use gpui::App;
use protocol::{ChannelType, Permissions, Snowflake};
use session::Command;

use super::community::access::GuildAccess;
use super::community::data_handle;
use crate::components::ContextMenuBuilder;
use crate::i18n::Translate as _;
use crate::icons::IconKind;

pub const REPORT_URL: &str = "https://support.discord.com/hc/en-us/requests/new";

pub fn open_report() {
	if let Err(error) = platform::open::open_url(REPORT_URL) {
		tracing::warn!(%error, "could not open the report page");
	}
}

pub fn user_items(
	menu: ContextMenuBuilder,
	guild: Snowflake,
	target: Snowflake,
	name: &str,
) -> ContextMenuBuilder {
	let name = name.to_owned();
	menu.lazy(move |cx| {
		let items = expand(guild, target, &name, cx);
		if items.is_empty() {
			items
		} else {
			ContextMenuBuilder::new().separator().extend(items)
		}
	})
}

pub fn expand(guild: Snowflake, target: Snowflake, name: &str, cx: &App) -> ContextMenuBuilder {
	let menu = ContextMenuBuilder::new();
	let data = data_handle(cx);
	let state = data.read(cx).state();
	let Some(me) = state.current_user_id() else {
		return menu;
	};
	let Some(found) = state.guild(guild) else {
		return menu;
	};
	let access = GuildAccess::of(&state, guild);
	let own = me == target;
	let outranks = !own && access.outranks(&state, guild, target);
	let member = found.members.get(&target);
	let current_nick = member.and_then(|member| member.nick.clone()).unwrap_or_default();
	let timed_out = member
		.and_then(|member| member.communication_disabled_until)
		.is_some_and(|until| until > Utc::now());
	let voice = found.voice_states.get(&target).cloned();
	let target_roles = member.map(|member| member.roles.clone()).unwrap_or_default();
	let mut roles: Vec<(Snowflake, String, i64)> = found
		.roles
		.values()
		.filter(|role| role.id != guild && !role.managed)
		.map(|role| (role.id, role.name.clone(), role.position))
		.collect();
	roles.sort_by_key(|(_, _, position)| std::cmp::Reverse(*position));
	let voice_channels: Vec<(Snowflake, String)> = state
		.guild_channel_tree(guild)
		.into_iter()
		.flat_map(|node| node.children)
		.filter(|channel| {
			!channel.locked
				&& matches!(
					channel.kind,
					ChannelType::GuildVoice | ChannelType::GuildStageVoice
				)
		})
		.map(|channel| (channel.id, channel.name))
		.collect();
	drop(state);
	let mut menu = menu;
	let can_nick = if own {
		access.has(Permissions::CHANGE_NICKNAME)
	} else {
		outranks && access.has(Permissions::MANAGE_NICKNAMES)
	};
	if can_nick {
		let nick = current_nick.clone();
		menu = menu.item(
			cx.t("mod-change-nickname"),
			Some(IconKind::Edit),
			move |window, cx| {
				dialogs::open_nickname(window, cx, guild, target, nick.clone());
			},
		);
	}
	if (outranks || own && access.owner) && access.has(Permissions::MANAGE_ROLES) {
		let mut submenu = ContextMenuBuilder::new();
		for (role_id, role_name, position) in roles {
			if !access.owner && position >= access.top_role {
				continue;
			}
			let has = target_roles.contains(&role_id);
			submenu = submenu.checked_item(role_name, has, move |_, cx| {
				let command = if has {
					Command::RemoveMemberRole {
						guild_id: guild,
						user_id: target,
						role_id,
					}
				} else {
					Command::AddMemberRole {
						guild_id: guild,
						user_id: target,
						role_id,
					}
				};
				data_handle(cx).read(cx).send(command);
			});
		}
		if !submenu.is_empty() {
			menu = menu.submenu(cx.t("mod-roles"), Some(IconKind::Shield), submenu);
		}
	}
	if let Some(voice) = voice.filter(|voice| voice.channel_id.is_some()) {
		if outranks && access.has(Permissions::MOVE_MEMBERS) {
			let mut submenu = ContextMenuBuilder::new();
			for (channel, channel_name) in voice_channels {
				let current = voice.channel_id == Some(channel);
				submenu = submenu.checked_item(channel_name, current, move |_, cx| {
					data_handle(cx).read(cx).send(Command::MoveMember {
						guild_id: guild,
						user_id: target,
						channel_id: Some(channel),
					});
				});
			}
			submenu = submenu
				.separator()
				.item(cx.t("mod-disconnect"), None, move |_, cx| {
					data_handle(cx).read(cx).send(Command::MoveMember {
						guild_id: guild,
						user_id: target,
						channel_id: None,
					});
				});
			menu = menu.submenu(cx.t("mod-move-to"), Some(IconKind::Volume), submenu);
		}
		if outranks && access.has(Permissions::MUTE_MEMBERS) {
			let muted = voice.mute;
			menu = menu.item(
				cx.t(if muted {
					"mod-server-unmute"
				} else {
					"mod-server-mute"
				}),
				Some(if muted { IconKind::Mic } else { IconKind::MicOff }),
				move |_, cx| {
					data_handle(cx).read(cx).send(Command::ServerMuteMember {
						guild_id: guild,
						user_id: target,
						muted: !muted,
					});
				},
			);
		}
		if outranks && access.has(Permissions::DEAFEN_MEMBERS) {
			let deafened = voice.deaf;
			menu = menu.item(
				cx.t(if deafened {
					"mod-server-undeafen"
				} else {
					"mod-server-deafen"
				}),
				Some(if deafened {
					IconKind::Headphones
				} else {
					IconKind::HeadphonesOff
				}),
				move |_, cx| {
					data_handle(cx).read(cx).send(Command::ServerDeafenMember {
						guild_id: guild,
						user_id: target,
						deafened: !deafened,
					});
				},
			);
		}
	}
	let mut punish = ContextMenuBuilder::new();
	if outranks && access.has(Permissions::MODERATE_MEMBERS) {
		if timed_out {
			punish = punish.item(cx.t("mod-remove-timeout"), Some(IconKind::Timer), move |_, cx| {
				data_handle(cx).read(cx).send(Command::TimeoutMember {
					guild_id: guild,
					user_id: target,
					until: None,
				});
			});
		} else {
			let label = name.to_owned();
			punish = punish.item(cx.t("mod-timeout"), Some(IconKind::Timer), move |window, cx| {
				dialogs::open_timeout(window, cx, guild, target, label.clone());
			});
		}
	}
	if outranks && access.has(Permissions::KICK_MEMBERS) {
		let label = name.to_owned();
		punish = punish.danger_item(cx.t("mod-kick"), Some(IconKind::UserX), move |window, cx| {
			dialogs::open_kick(window, cx, guild, target, label.clone());
		});
	}
	if outranks && access.has(Permissions::BAN_MEMBERS) {
		let label = name.to_owned();
		punish = punish.danger_item(cx.t("mod-ban"), Some(IconKind::Ban), move |window, cx| {
			dialogs::open_ban(window, cx, guild, target, label.clone());
		});
	}
	if punish.is_empty() {
		return menu;
	}
	if menu.is_empty() {
		punish
	} else {
		menu.separator().extend(punish)
	}
}
