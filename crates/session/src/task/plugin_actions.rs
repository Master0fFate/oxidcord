use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use plugin_host::api::model::{ClientOption, DmSection};
use plugin_host::{Invalidation, Navigation, Permission, PickedPath};
use protocol::{Permissions, Snowflake};
use state::{ClientOptions, DmSectionView, LoadDirection, LocalEvent, UiIntent};

use super::actions::{channel_notification_body, guild_notification_body};
use super::{Command, Core, Internal, PAGE_SIZE, page_query};
use crate::broker::is_known_dm;
use crate::command::{LoadResult, Reply};
use crate::persist::Job;
use crate::render::RenderJob;
use crate::views::message_meta;

const SAVE_LIMIT_BYTES: i64 = 100 * 1024 * 1024;
const MAX_INVALIDATED: usize = 1_024;

fn client_options_key(plugin_id: &str) -> String {
	format!("plugins.client-options.{plugin_id}")
}

fn option_state(option: ClientOption) -> ClientOptions {
	let mut options = ClientOptions::default();
	match option {
		ClientOption::MessageToolbarExpanded(value) => options.message_toolbar_expanded = Some(value),
		ClientOption::PlainFolderIcons(value) => options.plain_folder_icons = Some(value),
		ClientOption::FolderSidebar(value) => options.folder_sidebar = Some(value),
		ClientOption::ForumLayout(value) => options.forum_layout = Some(value),
		ClientOption::ForumSort(value) => options.forum_sort = Some(value),
		ClientOption::NotificationVolume(value) => options.notification_volume = Some(value),
		ClientOption::StreamerMode(value) => options.streamer_mode = Some(value),
		ClientOption::TypingAnimation(value) => options.typing_animation = Some(value),
		ClientOption::SuperReactDefault(value) => options.super_react_default = Some(value),
		ClientOption::ReplyPingDefault(value) => options.reply_ping_default = Some(value),
	}
	options
}

fn picked_path(path: PathBuf) -> Option<PickedPath> {
	let metadata = std::fs::metadata(&path).ok().filter(std::fs::Metadata::is_file)?;
	let name = path.file_name()?.to_string_lossy().into_owned();
	Some(PickedPath {
		content_type: Some(rest::content_type_for(&name).to_owned()),
		size: metadata.len(),
		name,
		path,
	})
}

fn suggested_name(name: &str) -> String {
	Path::new(name).file_name().map_or_else(
		|| "attachment".to_owned(),
		|name| name.to_string_lossy().into_owned(),
	)
}

impl Core {
	pub(super) fn on_plugin_action(&mut self, command: Command) {
		match command {
			Command::StartReply {
				channel_id,
				message_id,
				mention,
			} => self.push_intent(UiIntent::StartReply {
				channel_id,
				message_id,
				mention,
			}),
			Command::StartEdit {
				channel_id,
				message_id,
			} => self.push_intent(UiIntent::StartEdit {
				channel_id,
				message_id,
			}),
			Command::JumpToMessage {
				channel_id,
				message_id,
			} => self.push_intent(UiIntent::JumpToMessage {
				channel_id,
				message_id,
			}),
			Command::JumpToTop { channel_id } => self.jump_to_top(channel_id),
			Command::OpenSettings { page } => self.push_intent(UiIntent::OpenSettings(page)),
			Command::Navigate(Navigation::Back) => self.push_intent(UiIntent::NavigateBack),
			Command::Navigate(Navigation::Forward) => self.push_intent(UiIntent::NavigateForward),
			Command::PickFiles { multiple, reply } => self.pick_files(multiple, reply),
			Command::AttachFiles { channel_id, paths } => {
				self.push_intent(UiIntent::AttachFiles { channel_id, paths });
			}
			Command::AttachSticker {
				channel_id,
				sticker_id,
			} => self.push_intent(UiIntent::AttachSticker {
				channel_id,
				sticker_id,
			}),
			Command::SetComposerText { channel_id, content } => self.set_composer_text(channel_id, content),
			Command::SaveAttachment {
				channel_id,
				message_id,
				attachment_id,
			} => self.save_attachment(channel_id, message_id, attachment_id),
			Command::SetClientOption { plugin_id, option } => self.set_client_option(plugin_id, option),
			Command::SetDmSections { plugin_id, sections } => self.set_dm_sections(plugin_id, sections),
			Command::SetNotificationSettings {
				guild_id,
				channel_id,
				settings,
			} => {
				let (scope, body) = match channel_id {
					Some(channel_id) => (
						self.state.read().channel_guild(channel_id),
						channel_notification_body(channel_id, &settings),
					),
					None => (guild_id, guild_notification_body(&settings)),
				};
				if let Some(body) = body {
					self.spawn_action("notification settings", move |rest| async move {
						rest.patch_guild_settings(scope, body).await.map(drop)
					});
				}
			}
			Command::CloneEmoji {
				guild_id,
				emoji_id,
				animated,
				name,
			} => self.clone_emoji(guild_id, emoji_id, animated, name),
			Command::CloneSticker { guild_id, sticker_id } => self.clone_sticker(guild_id, sticker_id),
			_ => {}
		}
	}

	pub(super) fn push_intent(&mut self, intent: UiIntent) {
		self.apply_local(LocalEvent::UiIntent(intent));
	}

	fn set_composer_text(&mut self, channel_id: Snowflake, content: String) {
		self.persist.submit(Job::Draft {
			channel_id,
			content: content.clone(),
		});
		self.apply_local(LocalEvent::DraftChanged {
			channel_id,
			content: content.clone(),
		});
		self.push_intent(UiIntent::SetComposerText { channel_id, content });
	}

	fn jump_to_top(&mut self, channel_id: Snowflake) {
		let at_start = self
			.state
			.read()
			.messages(channel_id)
			.is_some_and(|cache| cache.reached_start);
		if at_start {
			self.push_intent(UiIntent::JumpToTop { channel_id });
			return;
		}
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let query = page_query(None, Some(Snowflake(0)), None);
			match rest.messages(channel_id, query).await {
				Ok(messages) => {
					let messages = messages.into_vec();
					let _ = internal.send(Internal::Messages {
						channel_id,
						direction: LoadDirection::Around,
						reached_start: messages.len() < usize::from(PAGE_SIZE),
						messages,
						cache: true,
					});
					let _ = internal.send(Internal::Intent(UiIntent::JumpToTop { channel_id }));
				}
				Err(error) => tracing::warn!(%error, "could not load the first messages"),
			}
		});
	}

	fn pick_files(&self, multiple: bool, reply: Reply<LoadResult<Vec<PickedPath>>>) {
		let sink = Arc::clone(&self.sink);
		self.runtime.spawn(async move {
			let picked = tokio::task::spawn_blocking(move || {
				sink.pick_files(multiple)
					.into_iter()
					.filter_map(picked_path)
					.collect()
			})
			.await
			.unwrap_or_default();
			reply.send(Ok(picked));
		});
	}

	fn save_attachment(&mut self, channel_id: Snowflake, message_id: Snowflake, attachment_id: Snowflake) {
		let message = self
			.state
			.read()
			.messages(channel_id)
			.and_then(|cache| cache.by_id.get(&message_id).cloned())
			.or_else(|| self.loads.fetched_message(channel_id, message_id));
		let target = message.as_ref().and_then(|message| {
			let attachment = message
				.attachments
				.iter()
				.find(|attachment| attachment.id == attachment_id)?;
			Some((
				attachment.url.clone()?,
				suggested_name(&attachment.filename),
				attachment.size.unwrap_or(0),
			))
		});
		let Some((url, name, size)) = target else {
			self.reject("That attachment is not available");
			return;
		};
		if size > SAVE_LIMIT_BYTES {
			self.reject("That attachment is larger than 100 MiB");
			return;
		}
		let Some(fetcher) = self.save_media.clone() else {
			self.reject("Downloads are unavailable");
			return;
		};
		let sink = Arc::clone(&self.sink);
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let chosen = tokio::task::spawn_blocking(move || sink.save_file(&name))
				.await
				.ok()
				.flatten();
			let Some(path) = chosen else {
				return;
			};
			let outcome = match fetcher.fetch(&url).await {
				Ok(bytes) => tokio::fs::write(&path, &bytes)
					.await
					.map_err(|error| error.to_string()),
				Err(error) => Err(error.to_string()),
			};
			if let Err(error) = outcome {
				tracing::warn!(%error, "attachment was not saved");
				let _ = internal.send(Internal::Rejected("The attachment was not saved".to_owned()));
			}
		});
	}

	fn options_of(&mut self, plugin_id: &str) -> ClientOptions {
		if let Some(options) = self.plugin_options.get(plugin_id) {
			return *options;
		}
		let stored = self
			.store
			.db
			.kv_get(&client_options_key(plugin_id))
			.ok()
			.flatten()
			.and_then(|bytes| serde_json::from_slice(&bytes).ok())
			.unwrap_or_default();
		self.plugin_options.insert(plugin_id.to_owned(), stored);
		stored
	}

	fn set_client_option(&mut self, plugin_id: String, option: ClientOption) {
		if !self.settings_plugins().contains(&plugin_id) {
			return;
		}
		let mut options = self.options_of(&plugin_id);
		options.overlay(&option_state(option));
		self.plugin_options.insert(plugin_id.clone(), options);
		let store = Arc::clone(&self.store);
		self.runtime.spawn_blocking(move || {
			let stored = serde_json::to_vec(&options)
				.map_err(|error| error.to_string())
				.and_then(|json| {
					store
						.db
						.kv_set(&client_options_key(&plugin_id), &json)
						.map_err(|error| error.to_string())
				});
			if let Err(error) = stored {
				tracing::warn!(target: "plugin", %error, "client options were not saved");
			}
		});
		self.refresh_plugin_shell();
	}

	fn set_dm_sections(&mut self, plugin_id: String, sections: Vec<DmSection>) {
		if !self.settings_plugins().contains(&plugin_id) {
			return;
		}
		let sections: Vec<DmSection> = {
			let state = self.state.read();
			sections
				.into_iter()
				.map(|section| {
					let mut channels: Vec<u64> = Vec::with_capacity(section.channels.len());
					for id in section.channels {
						if is_known_dm(&state, Snowflake(id)) && !channels.contains(&id) {
							channels.push(id);
						}
					}
					DmSection { channels, ..section }
				})
				.collect()
		};
		if sections.is_empty() {
			self.plugin_dm_sections.remove(&plugin_id);
		} else {
			self.plugin_dm_sections.insert(plugin_id, sections);
		}
		self.refresh_plugin_shell();
	}

	pub(super) fn forget_plugin_shell(&mut self, plugin_id: &str) {
		self.plugin_options.remove(plugin_id);
		self.plugin_dm_sections.remove(plugin_id);
		let store = Arc::clone(&self.store);
		let key = client_options_key(plugin_id);
		self.runtime.spawn_blocking(move || {
			if let Err(error) = store.db.kv_delete(&key) {
				tracing::warn!(target: "plugin", %error, "client options were not removed");
			}
		});
	}

	fn settings_plugins(&self) -> Vec<String> {
		let mut ids: Vec<String> = self
			.hub
			.manager
			.list()
			.into_iter()
			.filter(|plugin| {
				plugin.state.is_active() && plugin.granted.contains(&Permission::ActionsSettings)
			})
			.map(|plugin| plugin.id)
			.collect();
		ids.sort_unstable();
		ids
	}

	pub(super) fn refresh_plugin_shell(&mut self) {
		let active = self.settings_plugins();
		self.plugin_dm_sections
			.retain(|plugin_id, _| active.contains(plugin_id));
		let mut merged = ClientOptions::default();
		for plugin_id in &active {
			merged.overlay(&self.options_of(plugin_id));
		}
		let sections: Vec<DmSectionView> = self
			.plugin_dm_sections
			.iter()
			.filter(|(plugin_id, _)| active.contains(plugin_id))
			.flat_map(|(plugin_id, sections)| {
				sections.iter().map(|section| DmSectionView {
					plugin_id: plugin_id.clone(),
					title: section.title.clone(),
					channels: section.channels.iter().copied().map(Snowflake).collect(),
					collapsed: section.collapsed,
				})
			})
			.collect();
		self.apply_local(LocalEvent::ClientOptionsChanged(merged));
		self.apply_local(LocalEvent::DmSectionsChanged(sections));
	}

	fn clone_emoji(&mut self, guild_id: Snowflake, emoji_id: Snowflake, animated: bool, name: String) {
		if !self.guild_allows(
			guild_id,
			Permissions::CREATE_GUILD_EXPRESSIONS | Permissions::MANAGE_GUILD_EXPRESSIONS,
			"Create Expressions",
		) {
			return;
		}
		let Some(fetcher) = self.media.clone() else {
			self.reject("Downloads are unavailable");
			return;
		};
		let url = media::cdn::emoji_source_url(emoji_id.get(), animated);
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let message = match fetcher.fetch(&url).await {
				Ok(image) => Internal::Command(Box::new(Command::CreateEmoji {
					guild_id,
					name,
					image: image.to_vec(),
				})),
				Err(error) => {
					tracing::warn!(%error, "emoji image was not downloaded");
					Internal::Rejected("The emoji image could not be downloaded".to_owned())
				}
			};
			let _ = internal.send(message);
		});
	}

	fn clone_sticker(&mut self, guild_id: Snowflake, sticker_id: Snowflake) {
		if !self.guild_allows(
			guild_id,
			Permissions::CREATE_GUILD_EXPRESSIONS | Permissions::MANAGE_GUILD_EXPRESSIONS,
			"Create Expressions",
		) {
			return;
		}
		let sticker = self.state.read().find_sticker(sticker_id).cloned();
		let source = sticker.and_then(|sticker| {
			let format = media::cdn::StickerFormat::from(i64::from(sticker.format_type?));
			let extension = match format {
				media::cdn::StickerFormat::Gif => "gif",
				_ => "png",
			};
			Some((
				media::cdn::sticker_url(sticker.id.get(), format)?,
				extension,
				sticker,
			))
		});
		let (Some(fetcher), Some((url, extension, sticker))) = (self.media.clone(), source) else {
			self.reject("That sticker can't be cloned");
			return;
		};
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let message = match fetcher.fetch(&url).await {
				Ok(file) => Internal::Command(Box::new(Command::CreateSticker {
					guild_id,
					filename: format!("{}.{extension}", sticker.id),
					description: sticker.description.unwrap_or_default(),
					tags: sticker
						.tags
						.filter(|tags| !tags.is_empty())
						.unwrap_or_else(|| sticker.name.clone()),
					name: sticker.name,
					file: file.to_vec(),
				})),
				Err(error) => {
					tracing::warn!(%error, "sticker image was not downloaded");
					Internal::Rejected("The sticker image could not be downloaded".to_owned())
				}
			};
			let _ = internal.send(message);
		});
	}

	pub(super) fn on_invalidate(&mut self, plugin: &str, target: Invalidation) {
		tracing::debug!(target: "plugin", plugin, ?target, "plugin invalidation");
		match target {
			Invalidation::Message(channel_id, message_id) => {
				self.rerender_message(Snowflake(channel_id), Snowflake(message_id));
			}
			Invalidation::User(user_id) => {
				if self.invalidated_users.len() >= MAX_INVALIDATED {
					self.invalidated_users.clear();
				}
				self.invalidated_users.insert(Snowflake(user_id));
				self.schedule_overrides(Duration::ZERO);
			}
			Invalidation::Channel(channel_id) => self.decoration_stale(Snowflake(channel_id)),
		}
	}

	fn rerender_message(&mut self, channel_id: Snowflake, message_id: Snowflake) {
		if !self.flags.renders() {
			return;
		}
		let job = {
			let state = self.state.read();
			let Some(cache) = state.messages(channel_id) else {
				return;
			};
			let Some(message) = cache.by_id.get(&message_id).cloned() else {
				return;
			};
			RenderJob {
				guild_id: message.guild_id.or_else(|| state.channel_guild(channel_id)),
				epoch: self.epoch,
				deleted: cache.is_deleted(message_id),
				mine: state.current_user_id() == Some(message.author.id),
				intercept: self.flags.intercept,
				meta: self
					.flags
					.hooks
					.intercept_ext
					.then(|| message_meta(&state, Some(cache), &message)),
				message,
			}
		};
		self.render_worker.submit(vec![job]);
	}
}
