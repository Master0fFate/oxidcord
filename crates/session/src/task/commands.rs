use protocol::{
	CreateMessage, DispatchEvent, MessageAck, MessageReference, MessageReferenceKind, Permissions, Snowflake,
	StatusKind,
};
use rest::UploadFile;
use rest::api::image_data_uri;
use serde_json::{Value, json};
use state::{Loaded, LocalEvent, guild_folders_settings};

use super::actions::{mute_config, object, some, timestamp};
use super::{Command, Core, CustomStatus, Internal};

const PNG: &str = "image/png";
const DEFAULT_ARCHIVE_MINUTES: u32 = 1440;
const REACTION_PAGE: u8 = 100;
const BULK_DELETE_LIMIT: usize = 100;
const ACK_BULK_LIMIT: usize = 100;
const ARCHIVED_PAGE: u8 = 50;

fn sessions_or_empty(result: rest::Result<Value>) -> Value {
	result.unwrap_or_else(|error| {
		tracing::warn!(%error, "device sessions were not loaded");
		Value::Null
	})
}

fn png(bytes: &[u8]) -> Value {
	Value::String(image_data_uri(PNG, bytes))
}

fn forum_body(name: String, content: String, tags: Vec<Snowflake>, files: &[UploadFile]) -> Value {
	let attachments: Vec<Value> = files
		.iter()
		.enumerate()
		.map(|(index, file)| json!({ "id": index, "filename": file.name }))
		.collect();
	let mut message = json!({ "content": content });
	if !attachments.is_empty() {
		message["attachments"] = Value::Array(attachments);
	}
	json!({
		"name": name,
		"auto_archive_duration": DEFAULT_ARCHIVE_MINUTES,
		"applied_tags": tags,
		"message": message,
	})
}

impl Core {
	pub(super) fn on_extra(&mut self, command: Command) {
		match command {
			Command::StartCamera { device_id, quality } => self.start_camera(device_id, quality),
			Command::StopCamera => self.stop_camera(),
			Command::StartScreenShare {
				target,
				quality,
				with_audio,
			} => self.start_screen_share(target, quality, with_audio),
			Command::StopScreenShare => self.stop_screen_share(),
			Command::SetVideoQuality(quality) => self.set_video_quality(quality),
			Command::WatchStream { stream_key } => self.watch_stream(stream_key),
			Command::StopWatchingStream { stream_key } => self.stop_watching_stream(&stream_key),
			command @ (Command::CreateThread { .. }
			| Command::JoinThread { .. }
			| Command::LeaveThread { .. }
			| Command::ArchiveThread { .. }
			| Command::LoadArchivedThreads { .. }
			| Command::LoadActiveThreads { .. }
			| Command::CreateForumPost { .. }
			| Command::LoadPins { .. }
			| Command::PinMessage { .. }
			| Command::UnpinMessage { .. }
			| Command::LoadProfile { .. }
			| Command::CreateInvite { .. }
			| Command::ResolveInvite { .. }
			| Command::AcceptInvite { .. }
			| Command::LoadReactionUsers { .. }
			| Command::ForwardMessage { .. }
			| Command::VotePoll { .. }
			| Command::EndPoll { .. }) => self.on_text_command(command),
			command @ (Command::SendSticker { .. }
			| Command::LoadGifTrending
			| Command::LoadGifCategories
			| Command::LoadGifFavorites
			| Command::FavoriteGif { .. }
			| Command::UnfavoriteGif { .. }
			| Command::SearchGifs { .. }
			| Command::SuggestGifs { .. }
			| Command::SelectGif { .. }
			| Command::SendGif { .. }
			| Command::LoadStickerPacks
			| Command::LoadGuildStickers { .. }
			| Command::CreateSticker { .. }
			| Command::EditSticker { .. }
			| Command::DeleteSticker { .. }
			| Command::EditEmoji { .. }) => self.on_expression_command(command),
			command @ (Command::AddFriend { .. }
			| Command::AcceptFriend { .. }
			| Command::RemoveRelationship { .. }
			| Command::BlockUser { .. }
			| Command::LeaveGuild { .. }
			| Command::CreateGroupDm { .. }
			| Command::AddGroupRecipient { .. }
			| Command::RemoveGroupRecipient { .. }
			| Command::RenameGroup { .. }
			| Command::CloseDm { .. }
			| Command::MuteChannel { .. }
			| Command::UnmuteChannel { .. }
			| Command::MuteGuild { .. }
			| Command::UnmuteGuild { .. }
			| Command::SetChannelNotifications { .. }
			| Command::SetGuildNotifications { .. }
			| Command::MarkGuildRead { .. }
			| Command::MarkChannelRead { .. }
			| Command::MarkUnread { .. }
			| Command::UpdateUserSettings(_)
			| Command::SetGuildLayout(_)
			| Command::CreateGuild { .. }
			| Command::LoadSessions
			| Command::LogoutSessions(_)
			| Command::UpdateGuildSettingsRaw { .. }
			| Command::SetCustomStatus { .. }
			| Command::SetStatus(_)
			| Command::EditProfile { .. }
			| Command::SetGuildProfile { .. }) => self.on_social_command(command),
			command => self.on_admin_command(command),
		}
	}

	fn on_text_command(&mut self, command: Command) {
		match command {
			Command::CreateThread {
				channel_id,
				message_id,
				name,
				auto_archive_minutes,
			} => {
				if self.channel_allows(
					channel_id,
					Permissions::CREATE_PUBLIC_THREADS,
					"Create Public Threads",
				) {
					self.spawn_open("create thread", move |rest| async move {
						rest.start_thread(channel_id, message_id, &name, auto_archive_minutes)
							.await
					});
				}
			}
			Command::JoinThread { thread_id } => self.spawn_action("join thread", move |rest| async move {
				rest.join_thread(thread_id).await
			}),
			Command::LeaveThread { thread_id } => self.spawn_action("leave thread", move |rest| async move {
				rest.leave_thread(thread_id).await
			}),
			Command::ArchiveThread { thread_id, archived } => {
				self.spawn_action("archive thread", move |rest| async move {
					rest.modify_channel(thread_id, json!({ "archived": archived }))
						.await
						.map(drop)
				});
			}
			Command::LoadArchivedThreads { channel_id } => {
				let before = {
					let state = self.state.read();
					state
						.archived_threads_has_more(channel_id)
						.then(|| {
							state
								.archived_threads(channel_id)
								.last()
								.and_then(|thread| thread.thread_metadata.as_ref()?.archive_timestamp)
								.map(timestamp)
						})
						.flatten()
				};
				self.spawn_load("archived threads", move |rest| async move {
					let list = rest
						.threads_archived(channel_id, before.as_deref(), Some(ARCHIVED_PAGE))
						.await?;
					Ok(Loaded::ArchivedThreads {
						channel_id,
						threads: list.threads.into_vec(),
						members: list.members.into_vec(),
						has_more: list.has_more,
						append: before.is_some(),
					})
				});
			}
			Command::LoadActiveThreads { guild_id } => {
				self.spawn_load("active threads", move |rest| async move {
					let list = rest.threads_active(guild_id).await?;
					Ok(Loaded::ActiveThreads {
						guild_id,
						threads: list.threads.into_vec(),
						members: list.members.into_vec(),
					})
				});
			}
			Command::CreateForumPost {
				channel_id,
				name,
				content,
				tags,
				files,
			} => {
				if self.channel_allows(channel_id, Permissions::SEND_MESSAGES, "Send Messages") {
					let body = forum_body(name, content, tags, &files);
					self.spawn_open("create post", move |rest| async move {
						rest.create_forum_post(channel_id, body, files).await
					});
				}
			}
			Command::LoadPins { channel_id } => self.load_pins(channel_id),
			Command::PinMessage {
				channel_id,
				message_id,
			} => {
				if self.channel_allows(channel_id, Permissions::MANAGE_MESSAGES, "Manage Messages") {
					self.spawn_action("pin message", move |rest| async move {
						rest.pin_message(channel_id, message_id).await
					});
				}
			}
			Command::UnpinMessage {
				channel_id,
				message_id,
			} => {
				if self.channel_allows(channel_id, Permissions::MANAGE_MESSAGES, "Manage Messages") {
					self.spawn_action("unpin message", move |rest| async move {
						rest.unpin_message(channel_id, message_id).await
					});
				}
			}
			Command::LoadProfile { user_id, guild_id } => {
				self.spawn_load("profile", move |rest| async move {
					let raw = rest.user_profile(user_id, guild_id).await?;
					Ok(Loaded::Profile { user_id, raw })
				});
			}
			Command::CreateInvite {
				channel_id,
				max_age,
				max_uses,
				temporary,
			} => {
				if self.channel_allows(channel_id, Permissions::CREATE_INSTANT_INVITE, "Create Invite") {
					self.spawn_load("create invite", move |rest| async move {
						let options = json!({
							"max_age": max_age,
							"max_uses": max_uses,
							"temporary": temporary,
							"unique": true,
						});
						let raw = rest.create_invite(channel_id, options).await?;
						Ok(Loaded::InviteCreated { channel_id, raw })
					});
				}
			}
			Command::ResolveInvite { code } => self.spawn_load("invite", move |rest| async move {
				let raw = rest.invite(&code).await?;
				Ok(Loaded::InvitePreview { code, raw })
			}),
			Command::AcceptInvite { code } => self.spawn_action("accept invite", move |rest| async move {
				rest.accept_invite(&code).await.map(drop)
			}),
			Command::LoadReactionUsers {
				channel_id,
				message_id,
				emoji,
			} => self.spawn_load("reactions", move |rest| async move {
				let users = rest
					.reactions(channel_id, message_id, &emoji, REACTION_PAGE)
					.await?;
				Ok(Loaded::ReactionUsers {
					message_id,
					emoji,
					users: users.into_vec(),
				})
			}),
			Command::ForwardMessage {
				channel_id,
				message_id,
				to_channel_id,
			} => {
				let guild_id = self.state.read().channel_guild(channel_id);
				let body = CreateMessage {
					message_reference: Some(MessageReference {
						kind: Some(MessageReferenceKind::Forward),
						message_id: Some(message_id),
						channel_id: Some(channel_id),
						guild_id,
						fail_if_not_exists: None,
					}),
					..CreateMessage::default()
				};
				self.spawn_action("forward message", move |rest| async move {
					rest.create_message(to_channel_id, body).await.map(drop)
				});
			}
			Command::VotePoll {
				channel_id,
				message_id,
				answer_ids,
			} => self.spawn_action("vote", move |rest| async move {
				rest.vote_poll(channel_id, message_id, &answer_ids).await
			}),
			Command::EndPoll {
				channel_id,
				message_id,
			} => self.spawn_action("end poll", move |rest| async move {
				rest.end_poll(channel_id, message_id).await.map(drop)
			}),
			_ => {}
		}
	}

	pub(super) fn reload_pins(&self, channel_id: Snowflake) {
		if self.state.read().pins(channel_id).is_some() {
			self.load_pins(channel_id);
		}
	}

	pub(super) fn load_pins(&self, channel_id: Snowflake) {
		self.spawn_load("pins", move |rest| async move {
			let messages = rest.pins(channel_id).await?;
			Ok(Loaded::Pins {
				channel_id,
				messages: messages.into_vec(),
			})
		});
	}

	fn on_social_command(&mut self, command: Command) {
		match command {
			Command::AddFriend { username } => self.spawn_action("friend request", move |rest| async move {
				rest.send_friend_request(&username).await
			}),
			Command::AcceptFriend { user_id } => self.spawn_action("accept friend", move |rest| async move {
				rest.add_relationship(user_id).await
			}),
			Command::RemoveRelationship { user_id } => {
				self.spawn_action("remove friend", move |rest| async move {
					rest.remove_relationship(user_id).await
				});
			}
			Command::BlockUser { user_id } => {
				self.spawn_action(
					"block user",
					move |rest| async move { rest.block_user(user_id).await },
				)
			}
			Command::LeaveGuild { guild_id } => self.spawn_action("leave server", move |rest| async move {
				rest.leave_guild(guild_id).await
			}),
			Command::CreateGroupDm { recipient_ids } => {
				self.spawn_open("create group", move |rest| async move {
					rest.create_group_dm(&recipient_ids).await
				});
			}
			Command::AddGroupRecipient { channel_id, user_id } => {
				self.spawn_action("add to group", move |rest| async move {
					rest.add_group_recipient(channel_id, user_id).await
				});
			}
			Command::RemoveGroupRecipient { channel_id, user_id } => {
				self.spawn_action("remove from group", move |rest| async move {
					rest.remove_group_recipient(channel_id, user_id).await
				});
			}
			Command::RenameGroup { channel_id, name } => {
				self.spawn_action("rename group", move |rest| async move {
					rest.modify_channel(channel_id, json!({ "name": name }))
						.await
						.map(drop)
				});
			}
			Command::CloseDm { channel_id } => self
				.spawn_action("close conversation", move |rest| async move {
					rest.delete_channel(channel_id).await.map(drop)
				}),
			Command::MuteChannel {
				channel_id,
				guild_id,
				until,
			} => self.patch_channel_override(
				channel_id,
				guild_id,
				json!({ "muted": true, "mute_config": mute_config(true, until) }),
			),
			Command::UnmuteChannel { channel_id, guild_id } => {
				self.patch_channel_override(
					channel_id,
					guild_id,
					json!({ "muted": false, "mute_config": null }),
				);
			}
			Command::MuteGuild { guild_id, until } => self.patch_guild_mute(Some(guild_id), true, until),
			Command::UnmuteGuild { guild_id } => self.patch_guild_mute(Some(guild_id), false, None),
			Command::SetChannelNotifications { channel_id, level } => {
				let guild_id = self.state.read().channel_guild(channel_id);
				self.patch_channel_override(
					channel_id,
					guild_id,
					json!({ "message_notifications": i64::from(level) }),
				);
			}
			Command::SetGuildNotifications {
				guild_id,
				level,
				suppress_everyone,
				suppress_roles,
				mobile_push,
			} => {
				let body = json!({
					"message_notifications": i64::from(level),
					"suppress_everyone": suppress_everyone,
					"suppress_roles": suppress_roles,
					"mobile_push": mobile_push,
				});
				self.spawn_action("notification settings", move |rest| async move {
					rest.patch_guild_settings(guild_id, body).await.map(drop)
				});
			}
			Command::MarkGuildRead { guild_id } => self.mark_guild_read(guild_id),
			Command::MarkChannelRead { channel_id } => {
				let latest = self
					.state
					.read()
					.channel(channel_id)
					.and_then(|channel| channel.last_message_id);
				if let Some(message_id) = latest {
					self.ack(channel_id, message_id);
				}
			}
			Command::MarkUnread {
				channel_id,
				message_id,
			} => self.mark_unread(channel_id, message_id),
			Command::UpdateUserSettings(changes) => self
				.spawn_load("user settings", move |rest| async move {
					Ok(Loaded::UserSettings(rest.patch_user_settings(changes).await?))
				}),
			Command::SetGuildLayout(layout) => self.set_guild_layout(&layout),
			Command::CreateGuild { name, icon } => {
				self.select_created_guild = true;
				self.spawn_action("create server", move |rest| async move {
					rest.create_guild(&name, icon.as_deref()).await.map(drop)
				});
			}
			Command::LoadSessions => self.spawn_load("devices", |rest| async move {
				Ok(Loaded::AuthSessions(sessions_or_empty(
					rest.auth_sessions().await,
				)))
			}),
			Command::LogoutSessions(id_hashes) => self.spawn_load("log out devices", |rest| async move {
				rest.logout_sessions(&id_hashes).await?;
				Ok(Loaded::AuthSessions(sessions_or_empty(
					rest.auth_sessions().await,
				)))
			}),
			Command::UpdateGuildSettingsRaw { guild_id, changes } => {
				self.spawn_action("notification settings", move |rest| async move {
					rest.patch_guild_settings(guild_id, changes).await.map(drop)
				});
			}
			Command::SetCustomStatus { text, emoji, expires } => {
				let setting = (text.is_some() || emoji.is_some()).then(|| {
					json!({
						"text": text,
						"emoji_name": emoji,
						"expires_at": expires.map(timestamp),
					})
				});
				self.custom_status = text
					.clone()
					.or_else(|| emoji.clone())
					.map(|label| CustomStatus { text: label, emoji });
				self.publish_presence();
				self.spawn_action("custom status", move |rest| async move {
					rest.patch_user_settings(json!({ "custom_status": setting }))
						.await
						.map(drop)
				});
			}
			Command::SetStatus(status) => {
				let body = json!({ "status": status.as_str() });
				self.apply_local(LocalEvent::StatusChosen(status));
				self.spawn_action("status", move |rest| async move {
					rest.patch_user_settings(body).await.map(drop)
				});
			}
			Command::EditProfile {
				global_name,
				bio,
				avatar_png,
				banner_png,
				pronouns,
			} => {
				let account = object([
					("global_name", some(global_name)),
					("avatar", avatar_png.as_deref().map(png)),
				]);
				let profile = object([
					("bio", some(bio)),
					("pronouns", some(pronouns)),
					("banner", banner_png.as_deref().map(png)),
				]);
				self.spawn_action("edit profile", move |rest| async move {
					if account.as_object().is_some_and(|fields| !fields.is_empty()) {
						rest.patch_current_user(account).await?;
					}
					if profile.as_object().is_some_and(|fields| !fields.is_empty()) {
						rest.patch_current_profile(profile).await?;
					}
					Ok(())
				});
			}
			Command::SetGuildProfile {
				guild_id,
				nick,
				avatar_png,
				bio,
			} => {
				let body = object([
					("nick", some(nick)),
					("avatar", avatar_png.as_deref().map(png)),
					("bio", some(bio)),
				]);
				self.spawn_action("server profile", move |rest| async move {
					rest.modify_current_member(guild_id, body).await.map(drop)
				});
			}
			_ => {}
		}
	}

	fn mark_unread(&mut self, channel_id: Snowflake, message_id: Snowflake) {
		let previous = self
			.state
			.read()
			.messages(channel_id)
			.and_then(|cache| {
				let index = cache
					.ordered
					.iter()
					.position(|message| message.id == message_id)?;
				cache.ordered.get(index.checked_sub(1)?).map(|message| message.id)
			})
			.unwrap_or(Snowflake(message_id.0.saturating_sub(1)));
		self.apply_dispatch(&DispatchEvent::MessageAck(MessageAck {
			channel_id,
			message_id: previous,
			version: None,
			mention_count: Some(0),
		}));
		self.persist_read_state(channel_id);
		let body = json!({ "manual": true, "mention_count": 0 });
		self.spawn_rest("mark unread", move |rest| async move {
			rest.ack_message(channel_id, previous, body).await.map(drop)
		});
	}

	fn mark_guild_read(&mut self, guild_id: Snowflake) {
		let unread: Vec<(Snowflake, Snowflake)> = {
			let state = self.state.read();
			let Some(guild) = state.guild(guild_id) else {
				return;
			};
			guild
				.channels
				.values()
				.filter_map(|channel| {
					let last = channel.last_message_id?;
					let read = state
						.read_state(channel.id)
						.is_some_and(|read_state| read_state.last_message_id >= Some(last));
					(!read).then_some((channel.id, last))
				})
				.collect()
		};
		for (channel_id, message_id) in &unread {
			self.ack_local(*channel_id, *message_id);
		}
		for chunk in unread.chunks(ACK_BULK_LIMIT) {
			let chunk = chunk.to_vec();
			self.spawn_rest("ack guild", move |rest| async move { rest.ack_bulk(chunk).await });
		}
	}

	fn on_admin_command(&mut self, command: Command) {
		match command {
			Command::EditGuild {
				guild_id,
				name,
				icon_png,
				description,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					let body = object([
						("name", some(name)),
						("icon", icon_png.as_deref().map(png)),
						("description", some(description)),
					]);
					self.spawn_action("edit server", move |rest| async move {
						rest.modify_guild(guild_id, body).await.map(drop)
					});
				}
			}
			Command::CreateChannel {
				guild_id,
				name,
				kind,
				parent_id,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_CHANNELS, "Manage Channels") {
					let body = object([
						("name", Some(json!(name))),
						("type", Some(json!(i64::from(kind)))),
						("parent_id", parent_id.map(|id| json!(id))),
					]);
					self.spawn_action("create channel", move |rest| async move {
						rest.create_guild_channel(guild_id, body).await.map(drop)
					});
				}
			}
			Command::EditChannel {
				channel_id,
				name,
				topic,
				nsfw,
				slowmode,
				position,
				parent_id,
			} => {
				if self
					.channel_guild_allows(channel_id, Permissions::MANAGE_CHANNELS, "Manage Channels")
					.is_some()
				{
					let body = object([
						("name", some(name)),
						("topic", some(topic)),
						("nsfw", some(nsfw)),
						("rate_limit_per_user", some(slowmode)),
						("position", some(position)),
						("parent_id", parent_id.map(|id| json!(id))),
					]);
					self.spawn_action("edit channel", move |rest| async move {
						rest.modify_channel(channel_id, body).await.map(drop)
					});
				}
			}
			Command::DeleteChannel { channel_id } => {
				if self
					.channel_guild_allows(channel_id, Permissions::MANAGE_CHANNELS, "Manage Channels")
					.is_some()
				{
					self.spawn_action("delete channel", move |rest| async move {
						rest.delete_channel(channel_id).await.map(drop)
					});
				}
			}
			Command::CreateRole {
				guild_id,
				name,
				color,
				permissions,
				hoist,
				mentionable,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					let body = json!({
						"name": name,
						"color": color,
						"permissions": permissions,
						"hoist": hoist,
						"mentionable": mentionable,
					});
					self.spawn_action("create role", move |rest| async move {
						rest.create_role(guild_id, body).await.map(drop)
					});
				}
			}
			Command::EditRole {
				guild_id,
				role_id,
				name,
				color,
				permissions,
				hoist,
				mentionable,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					let body = object([
						("name", some(name)),
						("color", some(color)),
						("permissions", permissions.map(|permissions| json!(permissions))),
						("hoist", some(hoist)),
						("mentionable", some(mentionable)),
					]);
					self.spawn_action("edit role", move |rest| async move {
						rest.modify_role(guild_id, role_id, body).await.map(drop)
					});
				}
			}
			Command::DeleteRole { guild_id, role_id } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					self.spawn_action("delete role", move |rest| async move {
						rest.delete_role(guild_id, role_id).await
					});
				}
			}
			Command::ReorderRoles { guild_id, positions } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					self.spawn_action("reorder roles", move |rest| async move {
						rest.reorder_roles(guild_id, &positions).await.map(drop)
					});
				}
			}
			Command::SetChannelOverwrite {
				channel_id,
				target_id,
				kind,
				allow,
				deny,
			} => {
				if self
					.channel_guild_allows(channel_id, Permissions::MANAGE_ROLES, "Manage Roles")
					.is_some()
				{
					let body = json!({ "type": i64::from(kind), "allow": allow, "deny": deny });
					self.spawn_action("channel permissions", move |rest| async move {
						rest.set_channel_overwrite(channel_id, target_id, body).await
					});
				}
			}
			Command::DeleteChannelOverwrite {
				channel_id,
				target_id,
			} => {
				if self
					.channel_guild_allows(channel_id, Permissions::MANAGE_ROLES, "Manage Roles")
					.is_some()
				{
					self.spawn_action("channel permissions", move |rest| async move {
						rest.delete_channel_overwrite(channel_id, target_id).await
					});
				}
			}
			Command::CreateEmoji {
				guild_id,
				name,
				image_png,
			} => {
				if self.guild_allows(
					guild_id,
					Permissions::CREATE_GUILD_EXPRESSIONS,
					"Create Expressions",
				) {
					let image = image_data_uri(PNG, &image_png);
					self.spawn_action("create emoji", move |rest| async move {
						rest.create_emoji(guild_id, &name, &image).await.map(drop)
					});
				}
			}
			Command::DeleteEmoji { guild_id, emoji_id } => {
				if self.guild_allows(
					guild_id,
					Permissions::MANAGE_GUILD_EXPRESSIONS,
					"Manage Expressions",
				) {
					self.spawn_action("delete emoji", move |rest| async move {
						rest.delete_emoji(guild_id, emoji_id).await
					});
				}
			}
			Command::LoadAuditLog {
				guild_id,
				before,
				action_type,
			} => {
				if self.guild_allows(guild_id, Permissions::VIEW_AUDIT_LOG, "View Audit Log") {
					self.spawn_load("audit log", move |rest| async move {
						let raw = rest.audit_log(guild_id, before, action_type).await?;
						Ok(Loaded::AuditLog {
							guild_id,
							raw,
							append: before.is_some(),
						})
					});
				}
			}
			Command::LoadBans { guild_id } => {
				if self.guild_allows(guild_id, Permissions::BAN_MEMBERS, "Ban Members") {
					self.spawn_load("bans", move |rest| async move {
						let raw = rest.guild_bans(guild_id).await?;
						Ok(Loaded::Bans { guild_id, raw })
					});
				}
			}
			Command::LoadInvites { guild_id } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("invites", move |rest| async move {
						let raw = rest.guild_invites(guild_id).await?;
						Ok(Loaded::GuildInvites { guild_id, raw })
					});
				}
			}
			Command::RevokeInvite { guild_id, code } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("revoke invite", move |rest| async move {
						rest.delete_invite(&code, None).await?;
						let raw = rest.guild_invites(guild_id).await?;
						Ok(Loaded::GuildInvites { guild_id, raw })
					});
				}
			}
			command => self.on_moderation_command(command),
		}
	}

	fn on_moderation_command(&mut self, command: Command) {
		match command {
			Command::KickMember {
				guild_id,
				user_id,
				reason,
			} => {
				if self.guild_allows(guild_id, Permissions::KICK_MEMBERS, "Kick Members") {
					self.spawn_action("kick", move |rest| async move {
						rest.kick_member(guild_id, user_id, reason.as_deref()).await
					});
				}
			}
			Command::BanMember {
				guild_id,
				user_id,
				delete_message_seconds,
				reason,
			} => {
				if self.guild_allows(guild_id, Permissions::BAN_MEMBERS, "Ban Members") {
					self.spawn_action("ban", move |rest| async move {
						rest.ban_member(guild_id, user_id, delete_message_seconds, reason.as_deref())
							.await
					});
				}
			}
			Command::UnbanMember { guild_id, user_id } => {
				if self.guild_allows(guild_id, Permissions::BAN_MEMBERS, "Ban Members") {
					self.spawn_action("unban", move |rest| async move {
						rest.unban_member(guild_id, user_id).await
					});
				}
			}
			Command::TimeoutMember {
				guild_id,
				user_id,
				until,
			} => {
				if self.guild_allows(guild_id, Permissions::MODERATE_MEMBERS, "Timeout Members") {
					let body = json!({ "communication_disabled_until": until.map(timestamp) });
					self.modify_member(guild_id, user_id, body, "timeout");
				}
			}
			Command::SetNickname {
				guild_id,
				user_id,
				nick,
			} => {
				let me = self.state.read().current_user_id();
				let (needed, what) = if me == Some(user_id) {
					(Permissions::CHANGE_NICKNAME, "Change Nickname")
				} else {
					(Permissions::MANAGE_NICKNAMES, "Manage Nicknames")
				};
				if self.guild_allows(guild_id, needed, what) {
					if me == Some(user_id) {
						self.spawn_action("nickname", move |rest| async move {
							rest.modify_current_member(guild_id, json!({ "nick": nick }))
								.await
								.map(drop)
						});
					} else {
						self.modify_member(guild_id, user_id, json!({ "nick": nick }), "nickname");
					}
				}
			}
			Command::AddMemberRole {
				guild_id,
				user_id,
				role_id,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					self.spawn_action("add role", move |rest| async move {
						rest.add_member_role(guild_id, user_id, role_id).await
					});
				}
			}
			Command::RemoveMemberRole {
				guild_id,
				user_id,
				role_id,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					self.spawn_action("remove role", move |rest| async move {
						rest.remove_member_role(guild_id, user_id, role_id).await
					});
				}
			}
			Command::DeleteMessagesBulk { channel_id, ids } => {
				if self.channel_allows(channel_id, Permissions::MANAGE_MESSAGES, "Manage Messages") {
					for chunk in ids.chunks(BULK_DELETE_LIMIT) {
						let chunk = chunk.to_vec();
						self.spawn_action("delete messages", move |rest| async move {
							rest.bulk_delete_messages(channel_id, &chunk).await
						});
					}
				}
			}
			Command::MoveMember {
				guild_id,
				user_id,
				channel_id,
			} => {
				if self.guild_allows(guild_id, Permissions::MOVE_MEMBERS, "Move Members") {
					self.modify_member(guild_id, user_id, json!({ "channel_id": channel_id }), "move");
				}
			}
			Command::ServerMuteMember {
				guild_id,
				user_id,
				muted,
			} => {
				if self.guild_allows(guild_id, Permissions::MUTE_MEMBERS, "Mute Members") {
					self.modify_member(guild_id, user_id, json!({ "mute": muted }), "server mute");
				}
			}
			Command::ServerDeafenMember {
				guild_id,
				user_id,
				deafened,
			} => {
				let allowed = self.guild_allows(guild_id, Permissions::DEAFEN_MEMBERS, "Deafen Members");
				if allowed {
					self.modify_member(guild_id, user_id, json!({ "deaf": deafened }), "server deafen");
				}
			}
			_ => {}
		}
	}

	fn modify_member(&self, guild_id: Snowflake, user_id: Snowflake, body: Value, what: &'static str) {
		self.spawn_action(what, move |rest| async move {
			rest.modify_member(guild_id, user_id, body).await.map(drop)
		});
	}

	pub(super) fn publish_presence(&self) {
		let activities = self
			.custom_status
			.iter()
			.map(|custom| {
				let mut activity = json!({ "type": 4, "name": "Custom Status", "state": custom.text });
				if let Some(emoji) = &custom.emoji {
					activity["emoji"] = json!({ "name": emoji });
				}
				activity
			})
			.collect();
		let (status, afk) = {
			let state = self.state.read();
			(state.own_status(), state.is_afk())
		};
		if let Err(error) = self.gateway.send_presence(status.as_str(), activities, afk) {
			tracing::warn!(%error, "presence was not sent");
		}
	}

	pub(super) fn set_presence_command(&mut self, status: StatusKind, custom_status: Option<String>) {
		self.custom_status = custom_status.map(|text| CustomStatus { text, emoji: None });
		self.apply_local(LocalEvent::StatusChosen(status));
	}
}

impl Core {
	fn set_guild_layout(&mut self, layout: &[state::RailEntry]) {
		let previous = guild_folders_settings(&self.state.read().guild_layout());
		let body = guild_folders_settings(layout);
		self.apply_local(LocalEvent::Loaded(Box::new(Loaded::UserSettings(body.clone()))));
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			if let Err(error) = rest.patch_user_settings(body).await {
				tracing::warn!(%error, "rest call failed");
				let _ = internal.send(Internal::Loaded(Loaded::UserSettings(previous)));
				let _ = internal.send(Internal::Rejected("server order failed".to_owned()));
			}
		});
	}
}
