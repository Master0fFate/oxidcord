use std::time::Instant;

use activity::ActivityRules;
use chrono::{DateTime, Utc};
use protocol::rest::{ComponentInteraction, ComponentInteractionData, ModalSubmission, ModalSubmissionData};
use protocol::{
	ApplicationCommand, Channel, ChannelFlags, ChannelPosition, ChannelType, CommandAutocomplete,
	CreateMessage, DispatchEvent, InteractionOption, MessageAck, MessageFlags, MessageReference,
	MessageReferenceKind, Nonce, PermissionOverwriteKind, Permissions, ProtoCustomStatus,
	SCHEDULED_EVENT_GUILD_ONLY, Snowflake, StatusKind, ThreadMember,
};
use rest::api::ArchivedScope;
use rest::api::image_data_uri;
use rest::{RestClient, RestError, UploadFile};
use serde_json::{Value, json};
use state::{Loaded, LocalEvent, USER_COMMANDS, guild_layout_patch};

use super::actions::{mute_config, object, rest_failure, some, timestamp};
use super::{Command, Core, Internal, ReadSnapshot};
use crate::command::{
	ComponentUse, ForumEdit, ForumTagEdit, GuildEdit, ModalSubmit, RoleColorsEdit, RoleIconEdit,
};
use crate::persist::Job;

const COMPONENT_INTERACTION: u8 = 3;
const APPLICATION_COMMAND_INTERACTION: u8 = 2;
const AUTOCOMPLETE_INTERACTION: u8 = 4;
const MODAL_SUBMIT_INTERACTION: u8 = 5;

const PNG: &str = "image/png";
const GIF: &str = "image/gif";
const DEFAULT_ARCHIVE_MINUTES: u32 = 1440;
const REACTION_PAGE: u8 = 100;
const ACK_BULK_LIMIT: usize = 100;
const BAN_PAGE_LIMIT: usize = 1000;
const ARCHIVED_PAGE: u8 = 50;
const MENTIONS_PAGE: u8 = 25;
const PINS_PAGE: u8 = 50;

struct CommandCall<'a> {
	kind: u8,
	guild_id: Option<Snowflake>,
	channel_id: Snowflake,
	session_id: String,
	nonce: Nonce,
	command: &'a ApplicationCommand,
	target_id: Option<Snowflake>,
	options: &'a [InteractionOption],
}

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

async fn confirmed_profile(
	rest: &RestClient,
	user_id: Snowflake,
	guild_id: Option<Snowflake>,
) -> rest::Result<Loaded> {
	let raw = rest.user_profile(user_id, guild_id).await?;
	Ok(Loaded::Profile {
		user_id,
		guild_id,
		raw,
	})
}

fn private_overwrites(guild_id: Snowflake, creator: Option<Snowflake>) -> Value {
	let mut overwrites = vec![json!({
		"id": guild_id,
		"type": i64::from(PermissionOverwriteKind::Role),
		"allow": Permissions::empty(),
		"deny": Permissions::VIEW_CHANNEL,
	})];
	if let Some(creator) = creator {
		overwrites.push(json!({
			"id": creator,
			"type": i64::from(PermissionOverwriteKind::Member),
			"allow": Permissions::VIEW_CHANNEL,
			"deny": Permissions::empty(),
		}));
	}
	Value::Array(overwrites)
}

fn guild_edit_body(edit: GuildEdit) -> Value {
	object([
		("name", some(edit.name)),
		("description", some(edit.description)),
		("icon", edit.icon_png.as_deref().map(png)),
		("banner", edit.banner_png.as_deref().map(png)),
		("splash", edit.splash_png.as_deref().map(png)),
		("system_channel_id", edit.system_channel_id.map(|id| json!(id))),
		(
			"system_channel_flags",
			edit.system_channel_flags.map(|flags| json!(flags)),
		),
		("afk_channel_id", edit.afk_channel_id.map(|id| json!(id))),
		("afk_timeout", some(edit.afk_timeout)),
		(
			"default_message_notifications",
			edit.default_notifications.map(|level| json!(i64::from(level))),
		),
		(
			"verification_level",
			edit.verification_level.map(|level| json!(i64::from(level))),
		),
		(
			"explicit_content_filter",
			edit.explicit_content_filter
				.map(|filter| json!(i64::from(filter))),
		),
	])
}

fn forum_tag_body(tag: &ForumTagEdit) -> Value {
	object([
		("id", tag.id.map(|id| json!(id))),
		("name", Some(json!(tag.name))),
		("moderated", Some(json!(tag.moderated))),
		("emoji_id", Some(json!(tag.emoji_id))),
		("emoji_name", Some(json!(tag.emoji_name))),
	])
}

fn forum_edit_fields(forum: ForumEdit, flags: ChannelFlags) -> [(&'static str, Option<Value>); 6] {
	[
		(
			"available_tags",
			forum
				.available_tags
				.map(|tags| tags.iter().map(forum_tag_body).collect()),
		),
		(
			"default_reaction_emoji",
			forum.default_reaction.map(|reaction| json!(reaction)),
		),
		(
			"default_sort_order",
			forum.default_sort_order.map(|order| json!(i64::from(order))),
		),
		(
			"default_forum_layout",
			forum.default_layout.map(|layout| json!(i64::from(layout))),
		),
		(
			"default_thread_rate_limit_per_user",
			some(forum.default_thread_slowmode),
		),
		(
			"flags",
			forum.require_tag.map(|required| {
				let mut flags = flags;
				flags.set(ChannelFlags::REQUIRE_TAG, required);
				json!(flags)
			}),
		),
	]
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

async fn every_ban(rest: &RestClient, guild_id: Snowflake) -> rest::Result<Value> {
	let mut bans = Vec::new();
	let mut after = None;
	loop {
		let Value::Array(page) = rest.guild_bans(guild_id, after, BAN_PAGE_LIMIT).await? else {
			break;
		};
		let last = page
			.last()
			.and_then(|ban| ban["user"]["id"].as_str())
			.and_then(|id| id.parse::<u64>().ok())
			.map(Snowflake);
		let full = page.len() >= BAN_PAGE_LIMIT;
		bans.extend(page);
		match last {
			Some(last) if full && after != Some(last) => after = Some(last),
			_ => break,
		}
	}
	Ok(Value::Array(bans))
}

fn role_icon_fields(icon: Option<RoleIconEdit>) -> (Option<Value>, Option<Value>) {
	match icon {
		None => (None, None),
		Some(RoleIconEdit::Remove) => (Some(Value::Null), Some(Value::Null)),
		Some(RoleIconEdit::Image(bytes)) => (Some(png(&bytes)), Some(Value::Null)),
		Some(RoleIconEdit::Emoji(glyph)) => (Some(Value::Null), Some(Value::String(glyph))),
	}
}

impl Core {
	fn role_colors_field(&self, guild_id: Snowflake, colors: RoleColorsEdit) -> (&'static str, Value) {
		let enhanced = self
			.state
			.read()
			.guild(guild_id)
			.is_some_and(|guild| guild.has_feature(state::ENHANCED_ROLE_COLORS));
		if enhanced {
			(
				"colors",
				json!({
					"primary_color": colors.primary,
					"secondary_color": colors.secondary,
					"tertiary_color": colors.tertiary,
				}),
			)
		} else {
			("color", json!(colors.primary))
		}
	}

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
			Command::SetStreamVolume(volume) => self.set_stream_volume(volume),
			Command::WatchStream { stream_key } => self.watch_stream(stream_key),
			Command::StopWatchingStream { stream_key } => self.stop_watching_stream(&stream_key),
			Command::LoadStreamPreview { stream_key } => self.load_stream_preview(stream_key),
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
			| Command::LoadMentions { .. }
			| Command::DismissMention { .. }
			| Command::PinMessage { .. }
			| Command::UnpinMessage { .. }
			| Command::LoadProfile { .. }
			| Command::CreateInvite { .. }
			| Command::ResolveInvite { .. }
			| Command::AcceptInvite { .. }
			| Command::LoadMemberVerification { .. }
			| Command::SubmitMemberVerification { .. }
			| Command::WithdrawJoinRequest { .. }
			| Command::LoadReactionUsers { .. }
			| Command::ForwardMessage { .. }
			| Command::VotePoll { .. }
			| Command::EndPoll { .. }
			| Command::SendPoll { .. }
			| Command::UseComponent(_)
			| Command::SubmitModal(_)
			| Command::DismissEphemeral { .. }
			| Command::RunContextCommand { .. }
			| Command::RunChatCommand { .. }
			| Command::CompleteCommandOption { .. }
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
			| Command::LoadSoundboard { .. }
			| Command::LoadGuildSounds { .. }
			| Command::SendSound { .. }
			| Command::SendVoiceEffect { .. }
			| Command::CreateSound { .. }
			| Command::EditSound { .. }
			| Command::DeleteSound { .. }
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
			| Command::LoadRequestPreviews { .. }
			| Command::MuteChannel { .. }
			| Command::UnmuteChannel { .. }
			| Command::MuteGuild { .. }
			| Command::UnmuteGuild { .. }
			| Command::SetChannelNotifications { .. }
			| Command::MarkAllRead
			| Command::MarkGuildRead { .. }
			| Command::MarkChannelRead { .. }
			| Command::MarkUnread { .. }
			| Command::SetShowCurrentGame(_)
			| Command::SetPrivacy(_)
			| Command::SetSensitiveMedia { .. }
			| Command::SetGuildLayout(_)
			| Command::CreateGuild { .. }
			| Command::LoadSessions
			| Command::LoadConnections
			| Command::UpdateConnection { .. }
			| Command::RemoveConnection { .. }
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
				if self.channel_allows(channel_id, needed, what) && self.thread_cooldown_clear(channel_id) {
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
			Command::LoadArchivedThreads { channel_id, more } => {
				let private_scope = {
					let state = self.state.read();
					state
						.channel(channel_id)
						.is_some_and(|channel| channel.kind == ChannelType::GuildText)
						.then(|| {
							if state
								.my_permissions(channel_id)
								.contains(Permissions::MANAGE_THREADS)
							{
								ArchivedScope::Private
							} else {
								ArchivedScope::JoinedPrivate
							}
						})
				};
				self.load_archived_threads(channel_id, ArchivedScope::Public, more);
				if let Some(scope) = private_scope {
					self.load_archived_threads(channel_id, scope, more);
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
				if self.channel_allows(channel_id, Permissions::SEND_MESSAGES, "Send Messages")
					&& self.thread_cooldown_clear(channel_id)
				{
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
			Command::LoadMentions { roles, everyone } => {
				self.spawn_load("mentions", move |rest| async move {
					let messages = rest.mentions(MENTIONS_PAGE, roles, everyone).await?;
					Ok(Loaded::Mentions {
						roles,
						everyone,
						messages: messages.into_vec(),
					})
				})
			}
			Command::DismissMention { message_id } => {
				self.spawn_load("dismiss mention", move |rest| async move {
					rest.delete_mention(message_id).await?;
					Ok(Loaded::MentionDismissed(message_id))
				})
			}
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
					confirmed_profile(&rest, user_id, guild_id).await
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
			Command::ResolveInvite { code, event_id } => self.spawn_load("invite", move |rest| async move {
				let raw = rest.invite(&code, event_id).await?;
				Ok(Loaded::InvitePreview { code, raw })
			}),
			Command::AcceptInvite { code } => {
				let internal = self.internal.clone();
				self.spawn_action("accept invite", move |rest| async move {
					let accepted = rest.accept_invite(&code).await?;
					let Some(guild_id) = accepted.guild_id else {
						return Ok(());
					};
					if accepted.new_member {
						let _ = internal.send(Internal::SelectJoinedGuild(guild_id));
					}
					if !accepted.show_verification_form {
						return Ok(());
					}
					if let Some(guild) = accepted.guild {
						let _ = internal.send(Internal::Loaded(Loaded::Application {
							guild_id,
							name: guild.name.unwrap_or_default(),
							icon: guild.icon,
						}));
					}
					if let Ok(form) = rest.member_verification(guild_id, Some(&code)).await {
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
			Command::WithdrawJoinRequest { guild_id } => {
				self.spawn_action("withdraw application", move |rest| async move {
					rest.delete_join_request(guild_id).await
				});
			}
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
			Command::SubmitModal(submit) => self.submit_modal(submit),
			Command::DismissEphemeral {
				channel_id,
				message_id,
			} => {
				self.apply_local(LocalEvent::EphemeralDismissed {
					channel_id,
					message_id,
				});
				self.persist.submit(Job::DeleteMessages(vec![message_id]));
			}
			Command::RunContextCommand {
				guild_id,
				channel_id,
				target_id,
				command,
			} => self.run_context_command(guild_id, channel_id, target_id, &command),
			Command::RunChatCommand {
				guild_id,
				channel_id,
				command,
				options,
				files,
			} => self.run_chat_command(guild_id, channel_id, *command, options, files),
			Command::CompleteCommandOption {
				guild_id,
				channel_id,
				command,
				options,
				generation,
			} => self.complete_command_option(guild_id, channel_id, *command, options, generation),
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

	fn archived_cursor(&self, channel_id: Snowflake, scope: ArchivedScope) -> Option<String> {
		let state = self.state.read();
		let tail = state.archived_tail(channel_id, scope != ArchivedScope::Public)?;
		if scope == ArchivedScope::JoinedPrivate {
			return Some(tail.id.to_string());
		}
		tail.thread_metadata.as_ref()?.archive_timestamp.map(timestamp)
	}

	fn load_archived_threads(&self, channel_id: Snowflake, scope: ArchivedScope, more: bool) {
		let before = if more {
			let Some(cursor) = self.archived_cursor(channel_id, scope) else {
				return;
			};
			Some(cursor)
		} else {
			None
		};
		self.spawn_load("archived threads", move |rest| async move {
			let list = rest
				.threads_archived(channel_id, scope, before.as_deref(), Some(ARCHIVED_PAGE))
				.await?;
			Ok(Loaded::ArchivedThreads {
				channel_id,
				threads: list.threads.into_vec(),
				members: list.members.into_vec(),
				has_more: list.has_more,
				private: scope != ArchivedScope::Public,
				append: before.is_some(),
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
			Command::LoadRequestPreviews { channel_ids } => {
				self.spawn_load("message request previews", move |rest| async move {
					let previews = rest.message_request_previews(&channel_ids).await?;
					Ok(Loaded::RequestPreviews {
						channel_ids,
						previews,
					})
				})
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
			Command::MarkAllRead => {
				let unread = self.state.read().unread_targets();
				self.mark_read(&unread, "ack all");
			}
			Command::MarkGuildRead { guild_id } => {
				let unread = self.state.read().guild_unread_targets(guild_id);
				self.mark_read(&unread, "ack guild");
			}
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
				self.patch_settings_proto(patch, "activity sharing", None);
			}
			Command::SetPrivacy(change) => {
				let (patch, undo) = self.state.write().privacy_patch(change);
				self.apply_local(LocalEvent::SettingsEdited);
				self.patch_settings_proto(patch, "privacy", Some(undo));
			}
			Command::SetSensitiveMedia {
				category,
				scope,
				redaction,
			} => {
				let (patch, undo) = self.state.write().media_patch(category, scope, redaction);
				self.apply_local(LocalEvent::SettingsEdited);
				self.patch_settings_proto(patch, "sensitive media", Some(undo));
			}
			Command::SetGuildLayout(layout) => {
				self.set_guild_layout(layout);
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
			Command::LoadConnections => self.load_connections(),
			Command::UpdateConnection { kind, id, changes } => {
				self.spawn_load("update connection", move |rest| async move {
					Ok(Loaded::ConnectionUpdated(
						rest.modify_connection(&kind, &id, changes).await?,
					))
				})
			}
			Command::RemoveConnection { kind, id } => {
				self.spawn_load("remove connection", move |rest| async move {
					rest.delete_connection(&kind, &id).await?;
					Ok(Loaded::ConnectionRemoved { kind, id })
				})
			}
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
				self.patch_settings_proto(patch, "custom status", None);
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
				let Some(user_id) = self.state.read().current_user_id() else {
					return;
				};
				self.spawn_load("edit profile", move |rest| async move {
					if account.as_object().is_some_and(|fields| !fields.is_empty()) {
						rest.patch_current_user(account).await?;
					}
					if profile.as_object().is_some_and(|fields| !fields.is_empty()) {
						rest.patch_current_profile(profile).await?;
					}
					confirmed_profile(&rest, user_id, None).await
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
				let Some(user_id) = self.state.read().current_user_id() else {
					return;
				};
				self.spawn_load("server profile", move |rest| async move {
					rest.modify_current_member(guild_id, body).await?;
					confirmed_profile(&rest, user_id, Some(guild_id)).await
				});
			}
			_ => {}
		}
	}

	fn live_session_id(&mut self) -> Option<String> {
		let (session_id, connected) = {
			let state = self.state.read();
			let session = state.session();
			(session.session_id.clone(), session.connected)
		};
		if session_id.is_empty() || !connected {
			self.reject("Still connecting, try again shortly");
			return None;
		}
		Some(session_id)
	}

	fn command_body(&self, call: CommandCall) -> Value {
		let command = call.command;
		let mut body = json!({
			"type": call.kind,
			"application_id": command.application_id,
			"guild_id": call.guild_id,
			"channel_id": call.channel_id,
			"session_id": call.session_id,
			"nonce": call.nonce,
			"data": {
				"version": command.version,
				"id": command.id,
				"name": command.name,
				"type": command.kind,
				"application_command": command,
				"options": call.options,
				"attachments": [],
			},
		});
		if let Some(target_id) = call.target_id {
			body["data"]["target_id"] = json!(target_id);
		}
		body
	}

	fn run_context_command(
		&mut self,
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
		target_id: Snowflake,
		command: &ApplicationCommand,
	) {
		let Some(session_id) = self.live_session_id() else {
			return;
		};
		let nonce = self.next_nonce();
		self.track_interaction(&nonce, channel_id, None);
		let aborted = Internal::InteractionAborted(nonce.clone());
		let body = self.command_body(CommandCall {
			kind: APPLICATION_COMMAND_INTERACTION,
			guild_id,
			channel_id,
			session_id,
			nonce,
			command,
			target_id: Some(target_id),
			options: &[],
		});
		self.spawn_confirmed("run app command", Some(aborted), move |rest| async move {
			rest.create_interaction(body, Vec::new()).await
		});
	}

	fn run_chat_command(
		&mut self,
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
		command: ApplicationCommand,
		options: Vec<InteractionOption>,
		files: Vec<UploadFile>,
	) {
		let Some(session_id) = self.live_session_id() else {
			return;
		};
		let nonce = self.next_nonce();
		self.track_interaction(&nonce, channel_id, None);
		let aborted = Internal::InteractionAborted(nonce.clone());
		let body = self.command_body(CommandCall {
			kind: APPLICATION_COMMAND_INTERACTION,
			guild_id,
			channel_id,
			session_id,
			nonce,
			command: &command,
			target_id: None,
			options: &options,
		});
		self.spawn_confirmed("run app command", Some(aborted), move |rest| async move {
			rest.create_interaction(body, files).await
		});
	}

	fn complete_command_option(
		&mut self,
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
		command: ApplicationCommand,
		options: Vec<InteractionOption>,
		generation: u64,
	) {
		let Some(session_id) = self.live_session_id() else {
			return;
		};
		let nonce = self.next_nonce();
		self.completion = Some((nonce.clone(), generation));
		let body = self.command_body(CommandCall {
			kind: AUTOCOMPLETE_INTERACTION,
			guild_id,
			channel_id,
			session_id,
			nonce,
			command: &command,
			target_id: None,
			options: &options,
		});
		let rest = self.rest.clone();
		self.runtime.spawn(async move {
			if let Err(error) = rest.create_interaction(body, Vec::new()).await {
				tracing::debug!(%error, "command options could not be completed");
			}
		});
	}

	pub(super) fn on_command_choices(&mut self, response: CommandAutocomplete) {
		let Some((nonce, generation)) = &self.completion else {
			return;
		};
		if response.nonce.as_ref() != Some(nonce) {
			return;
		}
		let generation = *generation;
		self.on_loaded(Loaded::CommandChoices {
			generation,
			choices: response.choices.into_vec(),
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

	fn track_interaction(&mut self, nonce: &Nonce, channel_id: Snowflake, message_id: Option<Snowflake>) {
		self.apply_local(LocalEvent::InteractionStarted {
			nonce: nonce.clone(),
			channel_id,
			message_id,
		});
	}

	fn submit_modal(&mut self, submit: ModalSubmit) {
		let Some(session_id) = self.live_session_id() else {
			return;
		};
		let nonce = self.next_nonce();
		self.track_interaction(&nonce, submit.channel_id, None);
		let aborted = Internal::InteractionAborted(nonce.clone());
		let body = ModalSubmission {
			kind: MODAL_SUBMIT_INTERACTION,
			nonce: nonce.0,
			guild_id: self.state.read().channel_guild(submit.channel_id),
			channel_id: submit.channel_id,
			application_id: submit.application_id,
			session_id,
			data: ModalSubmissionData {
				id: submit.interaction_id,
				custom_id: submit.custom_id,
				components: submit.components,
			},
		};
		self.spawn_confirmed("submit modal", Some(aborted), move |rest| async move {
			rest.submit_modal(&body).await
		});
	}

	fn use_component(&mut self, used: ComponentUse) {
		let Some(session_id) = self.live_session_id() else {
			return;
		};
		let nonce = self.next_nonce();
		self.track_interaction(&nonce, used.channel_id, Some(used.message_id));
		let aborted = Internal::InteractionAborted(nonce.clone());
		let body = ComponentInteraction {
			kind: COMPONENT_INTERACTION,
			nonce: nonce.0,
			guild_id: used.guild_id,
			channel_id: used.channel_id,
			message_flags: used.message_flags,
			message_id: used.message_id,
			application_id: used.application_id,
			session_id,
			data: ComponentInteractionData {
				component_type: used.component_type,
				custom_id: used.custom_id,
				select_type: used.values.is_some().then_some(used.component_type),
				values: used.values,
			},
		};
		self.spawn_confirmed("use component", Some(aborted), move |rest| async move {
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

	pub(super) fn load_scheduled_events(&self, guild_id: Snowflake) {
		self.spawn_load("events", move |rest| async move {
			let requested = Instant::now();
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
				requested,
			})
		});
	}

	pub(super) fn load_connections(&self) {
		self.spawn_load("connections", |rest| async move {
			Ok(Loaded::Connections(rest.connections().await?.into_vec()))
		});
	}

	fn mark_read(&mut self, unread: &[(Snowflake, Snowflake)], what: &'static str) {
		let snapshots = self.read_snapshots(unread.iter().map(|(channel_id, _)| *channel_id));
		for (channel_id, message_id) in unread {
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
				what,
				Some(Internal::RestoreRead(restore)),
				move |rest| async move { rest.ack_bulk(chunk).await },
			);
		}
	}

	fn on_admin_command(&mut self, command: Command) {
		match command {
			Command::EditGuild { guild_id, edit } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					let body = guild_edit_body(edit);
					self.spawn_action("edit server", move |rest| async move {
						rest.modify_guild(guild_id, body).await.map(drop)
					});
				}
			}
			Command::DeleteGuild { guild_id } => {
				if self.owns_guild(guild_id) {
					self.spawn_action("delete server", move |rest| async move {
						rest.delete_guild(guild_id).await
					});
				} else {
					self.reject("Only the owner can delete a server");
				}
			}
			Command::RequestOwnershipCode { guild_id } => {
				if self.owns_guild(guild_id) {
					self.spawn_action("ownership code", move |rest| async move {
						rest.request_ownership_code(guild_id).await
					});
				} else {
					self.reject("Only the owner can transfer a server");
				}
			}
			Command::TransferGuild {
				guild_id,
				owner_id,
				code,
			} => {
				if self.owns_guild(guild_id) {
					self.spawn_action("transfer server", move |rest| async move {
						rest.transfer_guild(guild_id, owner_id, code.as_deref())
							.await
							.map(drop)
					});
				} else {
					self.reject("Only the owner can transfer a server");
				}
			}
			Command::CreateChannel {
				guild_id,
				name,
				kind,
				parent_id,
				private,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_CHANNELS, "Manage Channels")
					&& (!private || self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles"))
				{
					let creator = {
						let state = self.state.read();
						state.current_user_id().filter(|me| {
							!state
								.guild_permissions(guild_id, *me)
								.contains(Permissions::ADMINISTRATOR)
						})
					};
					let body = object([
						("name", Some(json!(name))),
						("type", Some(json!(i64::from(kind)))),
						("parent_id", parent_id.map(|id| json!(id))),
						(
							"permission_overwrites",
							private.then(|| private_overwrites(guild_id, creator)),
						),
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
				parent_id,
				voice,
				forum,
			} => {
				if self
					.channel_guild_allows(channel_id, Permissions::MANAGE_CHANNELS, "Manage Channels")
					.is_some()
				{
					let flags = self
						.state
						.read()
						.channel(channel_id)
						.map(|channel| channel.flags)
						.unwrap_or_default();
					let body = object(
						[
							("name", some(name)),
							("topic", some(topic)),
							("nsfw", some(nsfw)),
							("rate_limit_per_user", some(slowmode)),
							("parent_id", parent_id.map(|id| json!(id))),
							("bitrate", some(voice.bitrate)),
							("user_limit", some(voice.user_limit)),
							("rtc_region", voice.rtc_region.map(|region| json!(region))),
							(
								"video_quality_mode",
								voice.video_quality_mode.map(|mode| json!(i64::from(mode))),
							),
						]
						.into_iter()
						.chain(forum_edit_fields(forum, flags)),
					);
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
			Command::ReorderChannels { guild_id, positions } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_CHANNELS, "Manage Channels") {
					self.reorder_channels(guild_id, positions);
				}
			}
			Command::SetPostTags { thread_id, tags } => self.set_post_tags(thread_id, tags),
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
				colors,
				permissions,
				hoist,
				mentionable,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					let (colors_key, colors) = self.role_colors_field(guild_id, colors);
					let body = json!({
						"name": name,
						colors_key: colors,
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
				colors,
				icon,
				permissions,
				hoist,
				mentionable,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_ROLES, "Manage Roles") {
					let colors = colors.map(|colors| self.role_colors_field(guild_id, colors));
					let (icon, unicode_emoji) = role_icon_fields(icon);
					let mut body = object([
						("name", some(name)),
						("icon", icon),
						("unicode_emoji", unicode_emoji),
						("permissions", permissions.map(|permissions| json!(permissions))),
						("hoist", some(hoist)),
						("mentionable", some(mentionable)),
					]);
					if let Some((key, value)) = colors {
						body[key] = value;
					}
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
						let raw = every_ban(&rest, guild_id).await?;
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
			Command::SaveAutomodRule {
				guild_id,
				rule,
				original,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("save automod rule", move |rest| async move {
						let raw = rest
							.save_automod_rule(guild_id, &rule, original.as_deref())
							.await?;
						Ok(Loaded::AutomodRule { guild_id, raw })
					});
				}
			}
			Command::SetAutomodEnabled {
				guild_id,
				rule_id,
				enabled,
			} => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("save automod rule", move |rest| async move {
						let raw = rest.set_automod_rule_enabled(guild_id, rule_id, enabled).await?;
						Ok(Loaded::AutomodRule { guild_id, raw })
					});
				}
			}
			Command::DeleteAutomodRule { guild_id, rule_id } => {
				if self.guild_allows(guild_id, Permissions::MANAGE_GUILD, "Manage Server") {
					self.spawn_load("delete automod rule", move |rest| async move {
						rest.delete_automod_rule(guild_id, rule_id).await?;
						Ok(Loaded::AutomodRuleDeleted { guild_id, rule_id })
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
			Command::LoadScheduledEvents { guild_id } => self.load_scheduled_events(guild_id),
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
			Command::EditScheduledEvent {
				guild_id,
				event_id,
				name,
				description,
				channel_id,
				location,
				starts_at,
				ends_at,
			} => {
				let needed = self.event_permissions(guild_id, event_id);
				if self.guild_allows(guild_id, needed, "Manage Events") {
					let body = object([
						("name", Some(name.into())),
						("description", Some(description.map_or(Value::Null, Into::into))),
						("channel_id", channel_id.map(|id| json!(id))),
						(
							"entity_metadata",
							location.map(|location| json!({ "location": location })),
						),
						("scheduled_start_time", some(starts_at.map(timestamp))),
						("scheduled_end_time", some(ends_at.map(timestamp))),
					]);
					self.spawn_action("edit event", move |rest| async move {
						rest.modify_scheduled_event(guild_id, event_id, body)
							.await
							.map(drop)
					});
				}
			}
			Command::DeleteScheduledEvent { guild_id, event_id } => {
				let needed = self.event_permissions(guild_id, event_id);
				if self.guild_allows(guild_id, needed, "Manage Events") {
					self.spawn_action("delete event", move |rest| async move {
						rest.delete_scheduled_event(guild_id, event_id).await
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
		if (speaker || !own) && !self.channel_allows(channel_id, Permissions::MUTE_MEMBERS, "Mute Members") {
			return;
		}
		let body = json!({ "channel_id": channel_id, "suppress": !speaker });
		self.patch_voice_state(guild_id, (!own).then_some(user_id), body, "set speaker");
	}

	pub(super) fn patch_voice_state(
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

	fn patch_settings_proto(&self, patch: String, what: &'static str, undo: Option<LocalEvent>) {
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let message = match rest.patch_user_settings_proto(patch).await {
				Ok(encoded) => Internal::Loaded(Loaded::SettingsProto(encoded)),
				Err(error) => {
					if let Some(undo) = undo {
						let _ = internal.send(Internal::RestoreSettings(undo));
					}
					rest_failure(what, &error)
				}
			};
			let _ = internal.send(message);
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
	fn set_guild_layout(&mut self, layout: Vec<state::RailEntry>) {
		let previous = self.state.read().guild_layout();
		let patch = guild_layout_patch(&layout);
		self.apply_local(LocalEvent::GuildLayout(layout));
		self.patch_settings_proto(patch, "server order", Some(LocalEvent::GuildLayout(previous)));
	}

	pub(super) fn load_unnamed_applications(&self) {
		if self.state.read().has_unnamed_application() {
			self.spawn_load("applications", |rest| async move {
				Ok(Loaded::JoinRequestGuilds(rest.join_request_guilds().await?))
			});
		}
	}

	fn owns_guild(&self, guild_id: Snowflake) -> bool {
		let state = self.state.read();
		state
			.guild(guild_id)
			.is_some_and(|guild| guild.owner_id.is_some() && guild.owner_id == state.current_user_id())
	}

	fn reorder_channels(&mut self, guild_id: Snowflake, positions: Vec<ChannelPosition>) {
		let (previous, moved): (Vec<Channel>, Vec<Channel>) = {
			let state = self.state.read();
			positions
				.iter()
				.filter_map(|change| {
					let before = state.channel(change.id)?.clone();
					let mut after = before.clone();
					after.position = Some(change.position);
					if let Some(parent) = change.parent_id {
						after.parent_id = parent;
					}
					Some((before, after))
				})
				.unzip()
		};
		for channel in moved {
			self.apply_dispatch(&DispatchEvent::ChannelUpdate(Box::new(channel)));
		}
		let previous: Vec<_> = previous
			.iter()
			.map(|channel| (channel.id, channel.position, channel.parent_id))
			.collect();
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			if let Err(error) = rest.modify_channel_positions(guild_id, &positions).await {
				let placements = match rest.guild_channels(guild_id).await {
					Ok(fetched) => fetched
						.iter()
						.map(|channel| (channel.id, channel.position, channel.parent_id))
						.collect(),
					Err(_) => previous,
				};
				let _ = internal.send(Internal::RestoreChannels(placements));
				let _ = internal.send(rest_failure("reorder channels", &error));
			}
		});
	}

	fn set_post_tags(&mut self, thread_id: Snowflake, tags: Vec<Snowflake>) {
		let (owner, tag_required) = {
			let state = self.state.read();
			let thread = state.channel(thread_id);
			let forum = thread
				.and_then(|thread| thread.parent_id)
				.and_then(|parent| state.channel(parent));
			(
				thread.is_some_and(|thread| {
					thread.owner_id.is_some() && thread.owner_id == state.current_user_id()
				}),
				forum.is_some_and(|forum| forum.flags.contains(ChannelFlags::REQUIRE_TAG)),
			)
		};
		if tag_required && tags.is_empty() {
			self.reject("This forum requires a tag");
		} else if owner || self.channel_allows(thread_id, Permissions::MANAGE_THREADS, "Manage Threads") {
			let body = json!({ "applied_tags": tags });
			self.spawn_action("edit post tags", move |rest| async move {
				rest.modify_channel(thread_id, body).await.map(drop)
			});
		}
	}
}
