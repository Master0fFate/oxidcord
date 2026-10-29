use std::time::Duration;

use protocol::{CreateMessage, Gif, GifTrending, Permissions, Snowflake, Sticker, unicode_emoji_list};
use rest::UploadFile;
use serde::Serialize;
use state::{EmojiRef, Loaded, RECENT_CAP};

use super::commands::image_mime;
use super::outgoing::Outgoing;
use super::{Command, Core};
use crate::persist::{self, Job};

const GIF_MEDIA_FORMAT: &str = "gif";
const GIF_PAGE: u16 = 50;
const GIF_SUGGESTIONS: u8 = 10;
const RECENT_LOAD_TIMEOUT: Duration = Duration::from_secs(5);

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

fn reaction_emoji(emoji: &str) -> EmojiRef {
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

	fn may_manage_expressions(&mut self, guild_id: Snowflake) -> bool {
		self.guild_allows(
			guild_id,
			Permissions::MANAGE_GUILD_EXPRESSIONS,
			"Manage Expressions",
		)
	}

	pub(super) async fn restore_recents(&mut self) {
		let store = self.store.clone();
		let loading = tokio::task::spawn_blocking(move || {
			(
				persist::load_recent::<EmojiRef>(&store, persist::RECENT_EMOJI_KEY),
				persist::load_recent::<Gif>(&store, persist::RECENT_GIFS_KEY),
				persist::load_recent::<Sticker>(&store, persist::RECENT_STICKERS_KEY),
			)
		});
		let Ok(Ok((emoji, gifs, stickers))) = tokio::time::timeout(RECENT_LOAD_TIMEOUT, loading).await else {
			return;
		};
		if !emoji.is_empty() {
			self.on_loaded(Loaded::RecentEmoji(emoji));
		}
		if !gifs.is_empty() {
			self.on_loaded(Loaded::RecentGifs(gifs));
		}
		if !stickers.is_empty() {
			self.on_loaded(Loaded::RecentStickers(stickers));
		}
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
