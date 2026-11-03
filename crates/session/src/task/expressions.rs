use protocol::{
	CreateMessage, Gif, GifTrending, Permissions, SOUND_MAX_BYTES, SOUND_MAX_SECONDS, Snowflake, Sticker,
	unicode_emoji_list,
};
use rest::UploadFile;
use rest::api::image_data_uri;
use serde::Serialize;
use state::{EmojiRef, Loaded, RECENT_CAP, SoundBlock};

use super::actions::rest_failure;
use super::commands::image_mime;
use super::outgoing::Outgoing;
use super::{Command, Core, Internal};
use crate::persist::{self, Job};

const GIF_MEDIA_FORMAT: &str = "gif";
const GIF_PAGE: u16 = 50;
const GIF_SUGGESTIONS: u8 = 10;

fn promote<T>(list: &mut Vec<T>, item: T, is_same: impl Fn(&T, &T) -> bool) {
	list.retain(|entry| !is_same(entry, &item));
	list.insert(0, item);
	list.truncate(RECENT_CAP);
}

fn custom_emoji(content: &str) -> impl Iterator<Item = EmojiRef> {
	content.split('<').skip(1).filter_map(|tail| {
		let inner = tail.split_once('>')?.0;
		let (animated, rest) = match inner.strip_prefix("a:") {
			Some(rest) => (true, rest),
			None => (false, inner.strip_prefix(':')?),
		};
		let (name, id) = rest.split_once(':')?;
		Some(EmojiRef {
			name: name.to_owned(),
			id: Some(Snowflake(id.parse().ok()?)),
			animated,
		})
	})
}

fn unicode_emoji(content: &str) -> Vec<EmojiRef> {
	if content.is_ascii() {
		return Vec::new();
	}
	let mut found: Vec<(usize, &'static str)> = unicode_emoji_list()
		.iter()
		.filter_map(|entry| Some((content.find(entry.emoji)?, entry.emoji)))
		.collect();
	found.sort_unstable();
	found.dedup_by_key(|(_, glyph)| *glyph);
	found
		.into_iter()
		.map(|(_, glyph)| EmojiRef {
			name: glyph.to_owned(),
			id: None,
			animated: false,
		})
		.collect()
}

pub(super) fn reaction_emoji(emoji: &str) -> EmojiRef {
	let (animated, rest) = match emoji.strip_prefix("a:") {
		Some(rest) => (true, rest),
		None => (false, emoji),
	};
	let custom = rest.split_once(':').and_then(|(name, id)| {
		Some(EmojiRef {
			name: name.to_owned(),
			id: Some(Snowflake(id.parse().ok()?)),
			animated,
		})
	});
	custom.unwrap_or_else(|| EmojiRef {
		name: emoji.to_owned(),
		id: None,
		animated: false,
	})
}

enum SoundRefusal {
	Invalid(&'static str),
	Rest(rest::RestError),
}

async fn upload_sound(
	rest: &rest::RestClient,
	guild_id: Snowflake,
	name: &str,
	file: Vec<u8>,
) -> Result<(), SoundRefusal> {
	if file.len() > SOUND_MAX_BYTES {
		return Err(SoundRefusal::Invalid("Sounds can be at most 512 KB"));
	}
	let probe = file.clone();
	let decoded = tokio::task::spawn_blocking(move || voice::decode_sound(probe)).await;
	match decoded {
		Ok(Ok(sound)) if sound.duration().as_secs_f32() <= SOUND_MAX_SECONDS => {}
		Ok(Ok(_)) => return Err(SoundRefusal::Invalid("Sounds can be at most 5.2 seconds")),
		_ => return Err(SoundRefusal::Invalid("That audio file can't be used")),
	}
	let mime = if file.starts_with(b"OggS") {
		"audio/ogg"
	} else {
		"audio/mpeg"
	};
	rest.create_guild_sound(guild_id, name, &image_data_uri(mime, &file), 1.0)
		.await
		.map(drop)
		.map_err(SoundRefusal::Rest)
}

impl Core {
	pub(super) fn on_expression_command(&mut self, command: Command) {
		match command {
			Command::LoadGifTrending => {
				if self.state.read().gif_trending().is_some() {
					return;
				}
				let locale = self.locale.clone();
				self.spawn_load("gif trending", move |rest| async move {
					let (trending, gifs) = tokio::join!(
						rest.gifs_trending(&locale, GIF_MEDIA_FORMAT),
						rest.gifs_trending_gifs(&locale, GIF_MEDIA_FORMAT, GIF_PAGE),
					);
					Ok(Loaded::GifTrending(GifTrending {
						gifs: gifs?,
						..trending?
					}))
				});
			}
			Command::LoadGifFavorites => {
				if self.state.read().gif_favorites().is_some() {
					return;
				}
				self.spawn_load("gif favorites", move |rest| async move {
					Ok(Loaded::GifFavorites(rest.gif_favorites().await?))
				});
			}
			Command::FavoriteGif { gif } => self.spawn_load("favorite gif", move |rest| async move {
				Ok(Loaded::GifFavorites(rest.favorite_gif(&gif).await?))
			}),
			Command::UnfavoriteGif { url } => self.spawn_load("unfavorite gif", move |rest| async move {
				Ok(Loaded::GifFavorites(rest.unfavorite_gif(&url).await?))
			}),
			Command::SearchGifs { query } => {
				let locale = self.locale.clone();
				self.spawn_load("gif search", move |rest| async move {
					let gifs = rest
						.gifs_search(&query, &locale, GIF_MEDIA_FORMAT, GIF_PAGE)
						.await?
						.into_vec();
					Ok(Loaded::GifSearch { query, gifs })
				});
			}
			Command::SuggestGifs { query } => self.spawn_load("gif suggestions", move |rest| async move {
				let terms = rest.gifs_suggest(&query, GIF_SUGGESTIONS).await?;
				Ok(Loaded::GifSuggestions { query, terms })
			}),
			Command::SelectGif { id, query } => self.spawn_rest("select gif", move |rest| async move {
				rest.gif_select(&id, Some(query.as_str()).filter(|query| !query.is_empty()))
					.await
			}),
			Command::SendGif { channel_id, gif } => {
				let url = gif.url.clone();
				self.remember_gif(gif);
				self.send_message(channel_id, Outgoing::plain(url), None, None);
			}
			Command::SendSticker {
				channel_id,
				sticker_id,
			} => self.send_sticker(channel_id, sticker_id),
			Command::CreateSticker {
				guild_id,
				name,
				description,
				tags,
				file,
				filename,
			} => {
				if self.guild_allows(
					guild_id,
					Permissions::CREATE_GUILD_EXPRESSIONS | Permissions::MANAGE_GUILD_EXPRESSIONS,
					"Create Expressions",
				) {
					let file = UploadFile {
						name: filename,
						content_type: image_mime(&file).to_owned(),
						bytes: file.into(),
						description: None,
					};
					self.spawn_action("create sticker", move |rest| async move {
						rest.create_guild_sticker(guild_id, &name, &description, &tags, file)
							.await
							.map(drop)
					});
				}
			}
			Command::EditSticker {
				guild_id,
				sticker_id,
				name,
				description,
				tags,
			} => {
				if self.may_manage_expressions(guild_id) {
					self.spawn_action("edit sticker", move |rest| async move {
						rest.edit_guild_sticker(
							guild_id,
							sticker_id,
							name.as_deref(),
							description.as_deref(),
							tags.as_deref(),
						)
						.await
						.map(drop)
					});
				}
			}
			Command::DeleteSticker { guild_id, sticker_id } => {
				if self.may_manage_expressions(guild_id) {
					self.spawn_action("delete sticker", move |rest| async move {
						rest.delete_guild_sticker(guild_id, sticker_id).await
					});
				}
			}
			Command::EditEmoji {
				guild_id,
				id,
				name,
				roles,
			} => {
				if self.may_manage_expressions(guild_id) {
					self.spawn_action("edit emoji", move |rest| async move {
						rest.edit_guild_emoji(guild_id, id, Some(&name), Some(&roles))
							.await
							.map(drop)
					});
				}
			}
			Command::LoadSoundboard { guild_id } => self.load_soundboard(guild_id),
			Command::LoadGuildSounds { guild_id } => {
				self.spawn_load("server sounds", move |rest| async move {
					let sounds = rest.guild_sounds(guild_id).await?.items.into_vec();
					Ok(Loaded::GuildSounds { guild_id, sounds })
				})
			}
			Command::SendSound { channel_id, sound_id } => self.send_sound(channel_id, sound_id),
			Command::CreateSound { guild_id, name, file } => self.create_sound(guild_id, name, file),
			Command::EditSound {
				guild_id,
				sound_id,
				name,
				volume,
			} => {
				if self.may_change_sound(guild_id, sound_id) {
					self.spawn_action("edit sound", move |rest| async move {
						rest.edit_guild_sound(guild_id, sound_id, name.as_deref(), volume)
							.await
							.map(drop)
					});
				}
			}
			Command::DeleteSound { guild_id, sound_id } => {
				if self.may_change_sound(guild_id, sound_id) {
					self.spawn_action("delete sound", move |rest| async move {
						rest.delete_guild_sound(guild_id, sound_id).await
					});
				}
			}
			Command::LoadStickerPacks => self.spawn_load("sticker packs", move |rest| async move {
				let packs = rest.sticker_packs().await?;
				Ok(Loaded::StickerPacks(packs.sticker_packs.into_vec()))
			}),
			Command::LoadGuildStickers { guild_id } => {
				self.spawn_load("server stickers", move |rest| async move {
					let stickers = rest.guild_stickers(guild_id).await?.into_vec();
					Ok(Loaded::GuildStickers { guild_id, stickers })
				});
			}
			_ => {}
		}
	}

	fn load_soundboard(&mut self, guild_id: Snowflake) {
		let (needs_defaults, unloaded) = {
			let state = self.state.read();
			let wanted: Vec<Snowflake> = if state.has_nitro() {
				state.guilds().map(|guild| guild.id).collect()
			} else {
				vec![guild_id]
			};
			let unloaded: Vec<Snowflake> = wanted
				.into_iter()
				.filter(|id| state.guild_sounds(*id).is_none())
				.collect();
			(state.default_sounds().is_none(), unloaded)
		};
		if needs_defaults {
			self.spawn_load("default sounds", move |rest| async move {
				Ok(Loaded::DefaultSounds(rest.default_sounds().await?.into_vec()))
			});
		}
		if !unloaded.is_empty()
			&& let Err(error) = self.gateway.request_soundboard_sounds(unloaded)
		{
			tracing::warn!(%error, "soundboard request was not sent");
		}
	}

	fn send_sound(&mut self, channel_id: Snowflake, sound_id: Snowflake) {
		let (sound, block) = {
			let state = self.state.read();
			let sound = state.find_sound(sound_id).filter(|sound| sound.usable()).cloned();
			let block = sound.as_ref().and_then(|sound| state.sound_block(sound));
			(sound, block)
		};
		let Some(sound) = sound else {
			self.reject("That sound is unavailable");
			return;
		};
		if let Some(block) = block {
			self.reject(match block {
				SoundBlock::NotConnected => "Join the voice channel first",
				SoundBlock::NoPermission => "You need the Use Soundboard permission",
				SoundBlock::ExternalLocked => "You need the Use External Sounds permission",
				SoundBlock::Silenced => "You can't play sounds while muted or deafened",
			});
			return;
		}
		self.spawn_action("send sound", move |rest| async move {
			rest.send_sound(channel_id, sound_id, sound.guild_id).await
		});
	}

	fn create_sound(&mut self, guild_id: Snowflake, name: String, file: Vec<u8>) {
		if !self.guild_allows(
			guild_id,
			Permissions::CREATE_GUILD_EXPRESSIONS | Permissions::MANAGE_GUILD_EXPRESSIONS,
			"Create Expressions",
		) {
			return;
		}
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let message = match upload_sound(&rest, guild_id, &name, file).await {
				Ok(()) => return,
				Err(SoundRefusal::Rest(error)) => rest_failure("create sound", &error),
				Err(SoundRefusal::Invalid(reason)) => Internal::Rejected(reason.to_owned()),
			};
			let _ = internal.send(message);
		});
	}

	fn may_change_sound(&mut self, guild_id: Snowflake, sound_id: Snowflake) -> bool {
		let allowed = {
			let state = self.state.read();
			state
				.guild_sounds(guild_id)
				.and_then(|sounds| sounds.iter().find(|sound| sound.sound_id == sound_id))
				.is_some_and(|sound| state.may_change_sound(guild_id, sound))
		};
		if !allowed {
			self.reject("You need the Manage Expressions permission");
		}
		allowed
	}

	fn may_manage_expressions(&mut self, guild_id: Snowflake) -> bool {
		self.guild_allows(
			guild_id,
			Permissions::MANAGE_GUILD_EXPRESSIONS,
			"Manage Expressions",
		)
	}

	pub(super) fn restore_recents(&self) {
		let Some(db) = self.persist.database() else {
			return;
		};
		let internal = self.internal.clone();
		self.runtime.spawn_blocking(move || {
			let recents = [
				Loaded::RecentEmoji(persist::load_recent(&db, persist::RECENT_EMOJI_KEY)),
				Loaded::RecentGifs(persist::load_recent(&db, persist::RECENT_GIFS_KEY)),
				Loaded::RecentStickers(persist::load_recent(&db, persist::RECENT_STICKERS_KEY)),
			];
			for loaded in recents {
				let _ = internal.send(Internal::Loaded(loaded));
			}
		});
	}

	pub(super) fn remember_reaction(&mut self, emoji: &str) {
		self.remember_emoji(vec![reaction_emoji(emoji)]);
	}

	pub(super) fn remember_message_emoji(&mut self, content: &str) {
		let mut used: Vec<EmojiRef> = custom_emoji(content).collect();
		used.extend(unicode_emoji(content));
		self.remember_emoji(used);
	}

	fn remember_emoji(&mut self, used: Vec<EmojiRef>) {
		if used.is_empty() {
			return;
		}
		let mut recent = self.state.read().recent_emoji().to_vec();
		for emoji in used {
			promote(&mut recent, emoji, EmojiRef::is_same);
		}
		self.store_recent(persist::RECENT_EMOJI_KEY, &recent);
		self.on_loaded(Loaded::RecentEmoji(recent));
	}

	fn remember_gif(&mut self, gif: Gif) {
		let mut recent = self.state.read().recent_gifs().to_vec();
		promote(&mut recent, gif, |left, right| left.id == right.id);
		self.store_recent(persist::RECENT_GIFS_KEY, &recent);
		self.on_loaded(Loaded::RecentGifs(recent));
	}

	pub(super) fn send_sticker(&mut self, channel_id: Snowflake, sticker_id: Snowflake) {
		if !self.send_allowed(channel_id) {
			return;
		}
		self.note_stickers(&[sticker_id]);
		let body = CreateMessage {
			sticker_ids: vec![sticker_id],
			..CreateMessage::default()
		};
		self.spawn_action("send sticker", move |rest| async move {
			rest.create_message(channel_id, body).await.map(drop)
		});
	}

	pub(super) fn note_stickers(&mut self, stickers: &[Snowflake]) {
		for &sticker_id in stickers {
			let known = self.state.read().find_sticker(sticker_id).cloned();
			if let Some(sticker) = known {
				self.remember_sticker(sticker);
			}
		}
	}

	fn remember_sticker(&mut self, sticker: Sticker) {
		let mut recent = self.state.read().recent_stickers().to_vec();
		promote(&mut recent, sticker, |left, right| left.id == right.id);
		self.store_recent(persist::RECENT_STICKERS_KEY, &recent);
		self.on_loaded(Loaded::RecentStickers(recent));
	}

	fn store_recent<T: Serialize>(&self, key: &'static str, recent: &[T]) {
		match serde_json::to_vec(recent) {
			Ok(json) => self.persist.submit(Job::Recent { key, json }),
			Err(error) => tracing::warn!(%error, "recent expressions were not saved"),
		}
	}
}
