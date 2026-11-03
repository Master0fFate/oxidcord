use activity::ActivityRules;
use chrono::{DateTime, Utc};
use protocol::rest::{ComponentInteraction, ComponentInteractionData};
use protocol::{
	ApplicationCommand, ChannelType, CreateMessage, DispatchEvent, MessageAck, MessageFlags,
	MessageReference, MessageReferenceKind, Permissions, ProtoCustomStatus, SCHEDULED_EVENT_GUILD_ONLY,
	Snowflake, StatusKind, ThreadMember,
};
use rest::api::ArchivedScope;
use rest::api::image_data_uri;
use rest::{RestError, UploadFile};
use serde_json::{Value, json};
use state::{Loaded, LocalEvent, USER_COMMANDS, guild_folders_settings};

use super::actions::{mute_config, object, rest_failure, some, timestamp};
use super::{Command, Core, Internal, ReadSnapshot};
use crate::command::ComponentUse;

const COMPONENT_INTERACTION: u8 = 3;
const APPLICATION_COMMAND_INTERACTION: u8 = 2;

const PNG: &str = "image/png";
const GIF: &str = "image/gif";
const DEFAULT_ARCHIVE_MINUTES: u32 = 1440;
const REACTION_PAGE: u8 = 100;
const ACK_BULK_LIMIT: usize = 100;
const ARCHIVED_PAGE: u8 = 50;
const MENTIONS_PAGE: u8 = 25;
const PINS_PAGE: u8 = 50;

fn sessions_or_empty(result: rest::Result<Value>) -> Value {
	result.unwrap_or_else(|error| {
		tracing::warn!(%error, "device sessions were not loaded");
		Value::Null
	})
}

pub(super) fn image_mime(bytes: &[u8]) -> &'static str {
	if bytes.starts_with(b"GIF8") { GIF } else { PNG }
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
			Command::SetStreamQuality(quality) => self.streams.set_quality(quality),
			Command::WatchStream { stream_key } => self.watch_stream(stream_key),
			Command::StopWatchingStream { stream_key } => self.stop_watching_stream(&stream_key),
			command @ (Command::CreateThread { .. }
			| Command::JoinThread { .. }
			| Command::AddThreadMember { .. }
			| Command::LeaveThread { .. }
			| Command::ArchiveThread { .. }
			| Command::LockThread { .. }
			| Command::EditThread { .. }
			| Command::DeleteThread { .. }
			| Command::MuteThread { .. }
			| Command::UnmuteThread { .. }
			| Command::SetThreadNotifications { .. }
			| Command::LoadArchivedThreads { .. }
			| Command::LoadActiveThreads { .. }
			| Command::CreateForumPost { .. }
			| Command::LoadPins { .. }
			| Command::AckPins { .. }
			| Command::LoadMentions
			| Command::PinMessage { .. }
			| Command::UnpinMessage { .. }
			| Command::LoadProfile { .. }
			| Command::CreateInvite { .. }
			| Command::ResolveInvite { .. }
			| Command::AcceptInvite { .. }
			| Command::LoadMemberVerification { .. }
			| Command::SubmitMemberVerification { .. }
			| Command::LoadReactionUsers { .. }
			| Command::ForwardMessage { .. }
			| Command::VotePoll { .. }
			| Command::EndPoll { .. }
			| Command::SendPoll { .. }
			| Command::UseComponent(_)
			| Command::RunMessageCommand { .. }
			| Command::SaveMessage { .. }
			| Command::PublishMessage { .. }) => self.on_text_command(command),
			command @ (Command::SendSticker { .. }
			| Command::LoadGifTrending
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
			| Command::IgnoreUser { .. }
			| Command::UnignoreUser { .. }
			| Command::SetFriendNickname { .. }
			| Command::SetNote { .. }
			| Command::LeaveGuild { .. }
			| Command::CreateGroupDm { .. }
			| Command::AddGroupRecipient { .. }
			| Command::RemoveGroupRecipient { .. }
			| Command::RenameGroup { .. }
			| Command::SetGroupIcon { .. }
			| Command::TransferGroup { .. }
			| Command::CloseDm { .. }
			| Command::AcceptMessageRequest { .. }
			| Command::RejectMessageRequests { .. }
			| Command::MuteChannel { .. }
			| Command::UnmuteChannel { .. }
			| Command::MuteGuild { .. }
			| Command::UnmuteGuild { .. }
			| Command::SetChannelNotifications { .. }
			| Command::MarkGuildRead { .. }
			| Command::MarkChannelRead { .. }
			| Command::MarkUnread { .. }
			| Command::SetShowCurrentGame(_)
			| Command::SetFriendSources(_)
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
				private,
				invitable,
			} => {
				let (kind, needed, what) = self.thread_start(channel_id, message_id.is_none() && private);
				if self.channel_allows(channel_id, needed, what) {
					let invitable = (kind == ChannelType::PrivateThread).then_some(invitable);
					self.spawn_open("create thread", move |rest| async move {
						rest.start_thread(
							channel_id,
							message_id,
							&name,
							auto_archive_minutes,
							kind,
							invitable,
						)
						.await
					});
				}
			}
			Command::AddThreadMember { thread_id, user_id } => {
				self.spawn_action("add thread member", move |rest| async move {
					rest.add_thread_member(thread_id, user_id).await
				});
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
			Command::LockThread { thread_id, locked } => {
				if self.channel_allows(thread_id, Permissions::MANAGE_THREADS, "Manage Threads") {
					self.spawn_action("lock thread", move |rest| async move {
						rest.modify_channel(thread_id, json!({ "locked": locked }))
							.await
							.map(drop)
					});
				}
			}
			Command::EditThread {
				thread_id,
				name,
				slowmode,
				auto_archive_minutes,
			} => self.spawn_action("edit thread", move |rest| async move {
				let body = json!({
					"name": name,
					"rate_limit_per_user": slowmode,
					"auto_archive_duration": auto_archive_minutes,
				});
				rest.modify_channel(thread_id, body).await.map(drop)
			}),
			Command::DeleteThread { thread_id } => {
				if self.channel_allows(thread_id, Permissions::MANAGE_THREADS, "Manage Threads") {
					self.spawn_action("delete thread", move |rest| async move {
						rest.delete_channel(thread_id).await.map(drop)
					});
				}
			}
			Command::MuteThread { thread_id, until } => self.patch_thread_settings(
				thread_id,
				json!({ "muted": true, "mute_config": mute_config(true, until) }),
			),
			Command::UnmuteThread { thread_id } => {
				self.patch_thread_settings(thread_id, json!({ "muted": false, "mute_config": null }));
			}
			Command::SetThreadNotifications { thread_id, level } => {
				self.patch_thread_settings(thread_id, json!({ "flags": ThreadMember::level_flag(level) }))
			}
			Command::LoadArchivedThreads { channel_id } => {
				let (before, private_scope) = {
					let state = self.state.read();
					let before = state.archived_threads_cursor(channel_id).map(timestamp);
					let private_scope = (before.is_none()
						&& state
							.channel(channel_id)
							.is_some_and(|channel| channel.kind == ChannelType::GuildText))
					.then(|| {
						if state
							.my_permissions(channel_id)
							.contains(Permissions::MANAGE_THREADS)
						{
							ArchivedScope::Private
						} else {
							ArchivedScope::JoinedPrivate
						}
					});
					(before, private_scope)
				};
				self.load_archived_threads(channel_id, ArchivedScope::Public, before);
				if let Some(scope) = private_scope {
					self.load_archived_threads(channel_id, scope, None);
				}
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
			Command::LoadPins { channel_id, before } => self.load_pins(channel_id, before),
			Command::AckPins { channel_id } => {
				let pinned_at = self
					.state
					.read()
					.channel(channel_id)
					.and_then(|channel| channel.last_pin_timestamp);
				self.spawn_load("ack pins", move |rest| async move {
					rest.ack_pins(channel_id).await?;
					Ok(Loaded::PinsAcked {
						channel_id,
						pinned_at,
					})
				});
			}
			Command::LoadMentions => self.spawn_load("mentions", move |rest| async move {
				let messages = rest.mentions(MENTIONS_PAGE).await?;
				Ok(Loaded::Mentions(messages.into_vec()))
			}),
			Command::PinMessage {
				channel_id,
				message_id,
			} => {
				if self.channel_allows(channel_id, Permissions::PIN_MESSAGES, "Pin Messages") {
					self.spawn_action("pin message", move |rest| async move {
						rest.pin_message(channel_id, message_id).await
					});
				}
			}
			Command::UnpinMessage {
				channel_id,
				message_id,
			} => {
				if self.channel_allows(channel_id, Permissions::PIN_MESSAGES, "Pin Messages") {
					self.spawn_action("unpin message", move |rest| async move {
						rest.unpin_message(channel_id, message_id).await
					});
				}
			}
			Command::LoadProfile { user_id, guild_id } => {
				self.spawn_load("profile", move |rest| async move {
					let raw = rest.user_profile(user_id, guild_id).await?;
					Ok(Loaded::Profile {
						user_id,
						guild_id,
						raw,
					})
				});
				if self.state.read().current_user_id() != Some(user_id) {
					self.spawn_load("note", move |rest| async move {
						let note = rest.user_note(user_id).await?;
						Ok(Loaded::Note { user_id, note })
					});
				}
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
			Command::AcceptInvite { code } => {
				self.select_created_guild = true;
				let internal = self.internal.clone();
				self.spawn_action("accept invite", move |rest| async move {
					let accepted = rest.accept_invite(&code).await?;
					if let Some(guild_id) = accepted.guild_id.filter(|_| accepted.show_verification_form) {
						let form = rest.member_verification(guild_id, Some(&code)).await?;
						let _ =
							internal.send(Internal::Loaded(Loaded::MemberVerification { guild_id, form }));
					}
					Ok(())
				});
			}
			Command::LoadMemberVerification { guild_id } => {
				self.spawn_load("rules screening", move |rest| async move {
					let form = rest.member_verification(guild_id, None).await?;
					Ok(Loaded::MemberVerification { guild_id, form })
				})
			}
			Command::SubmitMemberVerification { guild_id, form } => self
				.spawn_action("rules screening", move |rest| async move {
					rest.submit_join_request(guild_id, &form).await
				}),
			Command::LoadReactionUsers {
				channel_id,
				message_id,
				emoji,
				burst,
			} => self.spawn_load("reactions", move |rest| async move {
				let users = rest
					.reactions(channel_id, message_id, &emoji, REACTION_PAGE, burst)
					.await?;
				Ok(Loaded::ReactionUsers {
					message_id,
					emoji,
					burst,
					users: users.into_vec(),
				})
			}),
			Command::ForwardMessage {
				channel_id,
				message_id,
				to_channel_id,
				comment,
				silent,
			} => {
				if !self.send_allowed(to_channel_id) {
					return;
				}
				let guild_id = self.state.read().channel_guild(channel_id);
				let flags = silent.then_some(MessageFlags::SUPPRESS_NOTIFICATIONS);
				let body = CreateMessage {
					message_reference: Some(MessageReference {
						kind: Some(MessageReferenceKind::Forward),
						message_id: Some(message_id),
						channel_id: Some(channel_id),
						guild_id,
						fail_if_not_exists: None,
					}),
					flags,
					..CreateMessage::default()
				};
				let internal = self.internal.clone();
				self.spawn_action("forward message", move |rest| async move {
					let sent = async {
						rest.create_message(to_channel_id, body).await?;
						if let Some(content) = comment {
							let body = CreateMessage {
								content: Some(content),
								flags,
								..CreateMessage::default()
							};
							rest.create_message(to_channel_id, body).await?;
						}
						Ok(())
					}
					.await;
					if let Err(RestError::Slowmode { retry_after }) = &sent {
						let _ = internal.send(Internal::SlowmodeHit {
							channel_id: to_channel_id,
							retry_after: *retry_after,
						});
					}
					sent
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
			Command::SendPoll { channel_id, poll } => {
				if self.channel_allows(channel_id, Permissions::SEND_POLLS, "Send Polls") {
					let body = CreateMessage {
						poll: Some(*poll),
						..CreateMessage::default()
					};
					self.spawn_action("send poll", move |rest| async move {
						rest.create_message(channel_id, body).await.map(drop)
					});
				}
			}
			Command::UseComponent(used) => self.use_component(used),
			Command::RunMessageCommand {
				guild_id,
				channel_id,
				message_id,
				command,
			} => self.run_message_command(guild_id, channel_id, message_id, *command),
			Command::SaveMessage {
				channel_id,
				message_id,
				due_at,
			} => self.spawn_action("bookmark message", move |rest| async move {
				let due_at = due_at.map(|due| due.to_rfc3339());
				rest.save_message(channel_id, message_id, due_at).await
			}),
			Command::PublishMessage {
				channel_id,
				message_id,
			} => self.spawn_action("publish message", move |rest| async move {
				rest.crosspost_message(channel_id, message_id).await
			}),
			_ => {}
		}
	}

	pub(super) fn reload_pins(&self, channel_id: Snowflake) {
		if self.state.read().pins(channel_id).is_some() {
			self.load_pins(channel_id, None);
		}
	}

	fn thread_start(&self, channel_id: Snowflake, private: bool) -> (ChannelType, Permissions, &'static str) {
		if private {
			return (
				ChannelType::PrivateThread,
				Permissions::CREATE_PRIVATE_THREADS,
				"Create Private Threads",
			);
		}
		let announcement = self
			.state
			.read()
			.channel(channel_id)
			.is_some_and(|channel| channel.kind == ChannelType::GuildAnnouncement);
		let kind = if announcement {
			ChannelType::AnnouncementThread
		} else {
			ChannelType::PublicThread
		};
		(kind, Permissions::CREATE_PUBLIC_THREADS, "Create Public Threads")
	}

	fn load_archived_threads(&self, channel_id: Snowflake, scope: ArchivedScope, before: Option<String>) {
		self.spawn_load("archived threads", move |rest| async move {
			let list = rest
				.threads_archived(channel_id, scope, before.as_deref(), Some(ARCHIVED_PAGE))
				.await?;
			Ok(Loaded::ArchivedThreads {
				channel_id,
				threads: list.threads.into_vec(),
				members: list.members.into_vec(),
				has_more: list.has_more,
				append: before.is_some(),
				private: scope != ArchivedScope::Public,
			})
		});
	}

	fn load_pins(&self, channel_id: Snowflake, before: Option<DateTime<Utc>>) {
		let before = before.map(timestamp);
		self.spawn_load("pins", move |rest| async move {
			let page = rest.pins(channel_id, before.as_deref(), PINS_PAGE).await?;
			Ok(Loaded::Pins {
				channel_id,
				items: page.items.into_vec(),
				has_more: page.has_more,
				append: before.is_some(),
			})
		});
	}

	fn on_social_command(&mut self, command: Command) {
		match command {
			Command::AddFriend {
				username,
				discriminator,
			} => {
				let rest = self.rest.clone();
				let internal = self.internal.clone();
				self.runtime.spawn(async move {
					let message = match rest.send_friend_request(&username, discriminator).await {
						Ok(()) => Internal::FriendRequestSent(username),
						Err(error) => match rest_failure("friend request", &error) {
							Internal::Rejected(reason) => Internal::FriendRequestFailed { username, reason },
							other => other,
						},
					};
					let _ = internal.send(message);
				});
			}
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
			Command::IgnoreUser { user_id } => self.spawn_action("ignore user", move |rest| async move {
				rest.set_ignored(user_id, true).await
			}),
			Command::UnignoreUser { user_id } => self.spawn_action("unignore user", move |rest| async move {
				rest.set_ignored(user_id, false).await
			}),
			Command::SetFriendNickname { user_id, nickname } => {
				self.spawn_action("friend nickname", move |rest| async move {
					rest.set_friend_nickname(user_id, nickname.as_deref()).await
				});
			}
			Command::SetNote { user_id, note } => self.spawn_action("save note", move |rest| async move {
				rest.set_user_note(user_id, &note).await
			}),
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
			Command::SetGroupIcon { channel_id, icon_png } => {
				self.spawn_action("change group icon", move |rest| async move {
					rest.modify_channel(channel_id, json!({ "icon": png(&icon_png) }))
						.await
						.map(drop)
				});
			}
			Command::TransferGroup { channel_id, owner_id } => {
				self.spawn_action("transfer group", move |rest| async move {
					rest.modify_channel(channel_id, json!({ "owner": owner_id }))
						.await
						.map(drop)
				});
			}
			Command::CloseDm { channel_id } => self
				.spawn_action("close conversation", move |rest| async move {
					rest.delete_channel(channel_id).await.map(drop)
				}),
			Command::AcceptMessageRequest { channel_id } => {
				self.spawn_action("accept message request", move |rest| async move {
					rest.accept_message_request(channel_id).await
				});
			}
			Command::RejectMessageRequests { channel_ids } => {
				self.spawn_action("ignore message requests", move |rest| async move {
					match channel_ids.as_slice() {
						[channel_id] => rest.reject_message_request(*channel_id).await,
						_ => rest.reject_message_requests(&channel_ids).await,
					}
				});
			}
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
			Command::SetShowCurrentGame(shown) => {
				let patch = self.state.write().show_current_game_patch(shown);
				self.patch_settings_proto(patch, "activity sharing");
			}
			Command::SetFriendSources(flags) => {
				let patch = self.state.write().friend_sources_patch(flags);
				self.patch_settings_proto(patch, "friend requests");
			}
			Command::SetGuildLayout(layout) => {
				self.set_guild_layout(&layout);
				self.note_list_changed();
			}
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
			Command::SetCustomStatus {
				text,
				emoji,
				emoji_id,
				expires,
			} => {
				let custom = (text.is_some() || emoji.is_some()).then(|| ProtoCustomStatus {
					text: text.unwrap_or_default(),
					emoji_id,
					emoji_name: emoji,
					expires_at_ms: expires.and_then(|at| u64::try_from(at.timestamp_millis()).ok()),
				});
				self.custom_status = None;
				let patch = self.state.write().custom_status_patch(custom.as_ref());
				self.patch_settings_proto(patch, "custom status");
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

	fn run_message_command(
		&mut self,
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
		message_id: Snowflake,
		command: ApplicationCommand,
	) {
		let body = json!({
			"type": APPLICATION_COMMAND_INTERACTION,
			"application_id": command.application_id,
			"guild_id": guild_id,
			"channel_id": channel_id,
			"session_id": self.state.read().session().session_id.clone(),
			"nonce": self.next_nonce().0,
			"data": {
				"version": command.version,
				"id": command.id,
				"name": command.name,
				"type": command.kind,
				"target_id": message_id,
				"application_command": command,
				"options": [],
				"attachments": [],
			},
		});
		self.spawn_action("run app command", move |rest| async move {
			rest.create_interaction(&body).await
		});
	}

	pub(super) fn load_commands(&mut self, guild_id: Option<Snowflake>, channel_id: Snowflake) {
		let scope = guild_id.unwrap_or(channel_id);
		for scope in [scope, USER_COMMANDS] {
			if !self.command_scopes.insert(scope) {
				continue;
			}
			let rest = self.rest.clone();
			let internal = self.internal.clone();
			self.runtime.spawn(async move {
				let index = match (scope == USER_COMMANDS, guild_id) {
					(true, _) => rest.user_command_index().await,
					(false, Some(guild_id)) => rest.guild_command_index(guild_id).await,
					(false, None) => rest.channel_command_index(channel_id).await,
				};
				match index {
					Ok(index) => {
						let _ = internal.send(Internal::Loaded(Loaded::CommandIndex { scope, index }));
					}
					Err(error) => tracing::debug!(%error, "app commands are not available here"),
				}
			});
		}
	}

	fn use_component(&mut self, used: ComponentUse) {
		let body = ComponentInteraction {
			kind: COMPONENT_INTERACTION,
			nonce: self.next_nonce().0,
			guild_id: used.guild_id,
			channel_id: used.channel_id,
			message_flags: used.message_flags,
			message_id: used.message_id,
			application_id: used.application_id,
			session_id: self.state.read().session().session_id.clone(),
			data: ComponentInteractionData {
				component_type: used.component_type,
				custom_id: used.custom_id,
				select_type: used.values.is_some().then_some(used.component_type),
				values: used.values,
			},
		};
		self.spawn_action("use component", move |rest| async move {
			rest.component_interaction(&body).await
		});
	}

	fn read_snapshots(&self, channels: impl Iterator<Item = Snowflake>) -> Vec<ReadSnapshot> {
		let state = self.state.read();
		channels
			.filter_map(|channel_id| {
				let read_state = state.read_state(channel_id)?;
				Some(ReadSnapshot {
					channel_id,
					message_id: read_state.last_message_id?,
					mention_count: read_state.mention_count,
				})
			})
			.collect()
	}

	fn mark_unread(&mut self, channel_id: Snowflake, message_id: Snowflake) {
		let restore = self.read_snapshots(std::iter::once(channel_id));
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
		self.spawn_confirmed(
			"mark unread",
			Some(Internal::RestoreRead(restore)),
			move |rest| async move { rest.ack_message(channel_id, previous, body).await.map(drop) },
		);
	}

	fn mark_guild_read(&mut self, guild_id: Snowflake) {
		let unread = self.state.read().guild_unread_targets(guild_id);
		let snapshots = self.read_snapshots(unread.iter().map(|(channel_id, _)| *channel_id));
		for (channel_id, message_id) in &unread {
			self.ack_local(*channel_id, *message_id);
		}
		for chunk in unread.chunks(ACK_BULK_LIMIT) {
			let chunk = chunk.to_vec();
			let restore = snapshots
				.iter()
				.filter(|snapshot| {
					chunk
						.iter()
						.any(|(channel_id, _)| *channel_id == snapshot.channel_id)
				})
				.copied()
				.collect();
			self.spawn_confirmed(
				"ack guild",
				Some(Internal::RestoreRead(restore)),
				move |rest| async move { rest.ack_bulk(chunk).await },
			);
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
				voice,
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
						("bitrate", some(voice.bitrate)),
						("user_limit", some(voice.user_limit)),
						("rtc_region", voice.rtc_region.map(|region| json!(region))),
						(
							"video_quality_mode",
							voice.video_quality_mode.map(|mode| json!(i64::from(mode))),
						),
					]);
					self.spawn_action("edit channel", move |rest| async move {
						rest.modify_channel(channel_id, body).await.map(drop)
					});
				}
			}
			Command::SetVoiceStatus { channel_id, status } => {
				if self.state.read().can_set_voice_status(channel_id) {
					self.spawn_action("set voice status", move |rest| async move {
						rest.set_voice_status(channel_id, status.as_deref()).await
					});
				} else {
					self.reject("You can't set the voice status");
				}
			}
			Command::LoadVoiceRegions => self.spawn_load("voice regions", move |rest| async move {
				Ok(Loaded::VoiceRegions(rest.voice_regions().await?))
			}),
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
				image,
			} => {
				if self.guild_allows(
					guild_id,
					Permissions::CREATE_GUILD_EXPRESSIONS | Permissions::MANAGE_GUILD_EXPRESSIONS,
					"Create Expressions",
				) {
					let image = image_data_uri(image_mime(&image), &image);
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
			Command::LoadAutomodRules { guild_id } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("automod rules", move |rest| async move {
						let raw = rest.automod_rules(guild_id).await?;
						Ok(Loaded::AutomodRules { guild_id, raw })
					});
				}
			}
			Command::SaveAutomodRule { guild_id, rule } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("save automod rule", move |rest| async move {
						rest.save_automod_rule(guild_id, &rule).await?;
						let raw = rest.automod_rules(guild_id).await?;
						Ok(Loaded::AutomodRules { guild_id, raw })
					});
				}
			}
			Command::DeleteAutomodRule { guild_id, rule_id } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("delete automod rule", move |rest| async move {
						rest.delete_automod_rule(guild_id, rule_id).await?;
						let raw = rest.automod_rules(guild_id).await?;
						Ok(Loaded::AutomodRules { guild_id, raw })
					});
				}
			}
			Command::LoadRoleCounts { guild_id } => {
				self.spawn_load("role counts", move |rest| async move {
					let raw = rest.role_member_counts(guild_id).await?;
					Ok(Loaded::RoleCounts { guild_id, raw })
				});
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
			Command::LoadScheduledEvents { guild_id } => {
				self.spawn_load("events", move |rest| async move {
					let events = rest.scheduled_events(guild_id).await?.into_vec();
					let interested = rest
						.scheduled_event_rsvps(guild_id)
						.await
						.map_err(|error| tracing::warn!(%error, "event interest was not loaded"))
						.ok()
						.map(|rsvps| {
							rsvps
								.iter()
								.filter(|rsvp| rsvp.is_interested())
								.map(|rsvp| rsvp.guild_scheduled_event_id)
								.collect()
						});
					Ok(Loaded::ScheduledEvents {
						guild_id,
						events,
						interested,
					})
				});
			}
			Command::CreateScheduledEvent {
				guild_id,
				name,
				description,
				kind,
				channel_id,
				location,
				starts_at,
				ends_at,
			} => {
				if self.guild_allows(
					guild_id,
					Permissions::CREATE_EVENTS | Permissions::MANAGE_EVENTS,
					"Create Events",
				) {
					let body = object([
						("name", Some(name.into())),
						("description", some(description)),
						("privacy_level", Some(SCHEDULED_EVENT_GUILD_ONLY.into())),
						("entity_type", Some(i64::from(kind).into())),
						("channel_id", channel_id.map(|id| json!(id))),
						(
							"entity_metadata",
							location.map(|location| json!({ "location": location })),
						),
						("scheduled_start_time", Some(timestamp(starts_at).into())),
						("scheduled_end_time", some(ends_at.map(timestamp))),
					]);
					self.spawn_action("create event", move |rest| async move {
						rest.create_scheduled_event(guild_id, body).await.map(drop)
					});
				}
			}
			Command::SetScheduledEventStatus {
				guild_id,
				event_id,
				status,
			} => {
				let needed = self.event_permissions(guild_id, event_id);
				if self.guild_allows(guild_id, needed, "Manage Events") {
					let body = json!({ "status": i64::from(status) });
					self.spawn_action("update event", move |rest| async move {
						rest.modify_scheduled_event(guild_id, event_id, body)
							.await
							.map(drop)
					});
				}
			}
			Command::SetScheduledEventInterest {
				guild_id,
				event_id,
				interested,
			} => {
				self.spawn_action("event interest", move |rest| async move {
					rest.set_scheduled_event_interest(guild_id, event_id, interested)
						.await
				});
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
					self.spawn_role_change("add role", guild_id, move |rest| async move {
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
					self.spawn_role_change("remove role", guild_id, move |rest| async move {
						rest.remove_member_role(guild_id, user_id, role_id).await
					});
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
			Command::StartStage { channel_id, topic } => {
				if self.stage_moderator(channel_id) {
					self.spawn_action("start stage", move |rest| async move {
						rest.create_stage_instance(channel_id, &topic).await.map(drop)
					});
				}
			}
			Command::EditStageTopic { channel_id, topic } => {
				if self.stage_moderator(channel_id) {
					self.spawn_action("edit stage topic", move |rest| async move {
						rest.modify_stage_instance(channel_id, &topic).await.map(drop)
					});
				}
			}
			Command::EndStage { channel_id } => {
				if self.stage_moderator(channel_id) {
					self.spawn_action("end stage", move |rest| async move {
						rest.delete_stage_instance(channel_id).await
					});
				}
			}
			Command::SetSpeaker {
				guild_id,
				channel_id,
				user_id,
				speaker,
			} => self.set_speaker(guild_id, channel_id, user_id, speaker),
			Command::AcceptSpeakInvite { guild_id, channel_id } => self.patch_voice_state(
				guild_id,
				None,
				json!({ "channel_id": channel_id, "suppress": false, "request_to_speak_timestamp": null }),
				"accept speak invite",
			),
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

	fn stage_moderator(&mut self, channel_id: Snowflake) -> bool {
		let needed = Permissions::MANAGE_CHANNELS | Permissions::MUTE_MEMBERS | Permissions::MOVE_MEMBERS;
		self.channel_allows(channel_id, needed, "Stage Moderator")
	}

	fn set_speaker(&mut self, guild_id: Snowflake, channel_id: Snowflake, user_id: Snowflake, speaker: bool) {
		let own = self.state.read().current_user_id() == Some(user_id);
		if (speaker || !own) && !self.guild_allows(guild_id, Permissions::MUTE_MEMBERS, "Mute Members") {
			return;
		}
		let body = json!({ "channel_id": channel_id, "suppress": !speaker });
		self.patch_voice_state(guild_id, (!own).then_some(user_id), body, "set speaker");
	}

	fn patch_voice_state(
		&self,
		guild_id: Snowflake,
		user_id: Option<Snowflake>,
		body: Value,
		what: &'static str,
	) {
		self.spawn_action(what, move |rest| async move {
			rest.modify_voice_state(guild_id, user_id, body).await
		});
	}

	fn modify_member(&self, guild_id: Snowflake, user_id: Snowflake, body: Value, what: &'static str) {
		self.spawn_action(what, move |rest| async move {
			rest.modify_member(guild_id, user_id, body).await.map(drop)
		});
	}

	fn patch_settings_proto(&self, patch: String, what: &'static str) {
		self.spawn_load(what, move |rest| async move {
			Ok(Loaded::SettingsProto(
				rest.patch_user_settings_proto(patch).await?,
			))
		});
	}

	pub(super) fn publish_presence(&self) {
		let (status, afk, saved) = {
			let state = self.state.read();
			(state.own_status(), state.is_afk(), state.custom_status().cloned())
		};
		let activities = self
			.custom_status
			.as_ref()
			.or(saved.as_ref())
			.map(custom_status_activity)
			.into_iter()
			.chain(self.activities.iter().cloned())
			.collect();
		if let Err(error) = self.gateway.send_presence(status.as_str(), activities, afk) {
			tracing::warn!(%error, "presence was not sent");
		}
	}

	pub(super) fn push_activity_rules(&self) {
		let show_current_game = self.state.read().show_current_game();
		let local = self.store.settings.read().activity;
		self.activity.set_rules(ActivityRules {
			show_current_game,
			rpc_server: local.rpc_server,
			detect_games: local.detect_games,
			spotify: local.spotify,
		});
	}

	pub(super) fn set_presence_command(&mut self, status: StatusKind, custom_status: Option<String>) {
		self.custom_status = custom_status.map(|text| ProtoCustomStatus {
			text,
			..ProtoCustomStatus::default()
		});
		self.apply_local(LocalEvent::StatusChosen(status));
	}
}

fn custom_status_activity(custom: &ProtoCustomStatus) -> Value {
	let text = Some(&custom.text).filter(|text| !text.is_empty());
	let mut activity = json!({ "type": 4, "name": "Custom Status", "state": text });
	if custom.emoji_name.is_some() || custom.emoji_id.is_some() {
		activity["emoji"] = json!({ "name": custom.emoji_name, "id": custom.emoji_id });
	}
	activity
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
