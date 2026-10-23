use anyhow::{Context, Result, ensure};
use mockcord::Fixtures;
use protocol::{ChannelType, Message, Snowflake};
use session::{Command, ScreenSource, ScreenTarget, ScreenTargetKind, VideoQuality};
use state::{MemberListRow, State};

use super::Harness;

const THREAD_NAME: &str = "headless thread";
const KICK_REASON: &str = "headless";
const AUDIT_KICK: i64 = 20;
const GROUP_MEMBERS: usize = 2;
const MOCK_USER_ID: u64 = 100;
const SHARE_WIDTH: u32 = 1280;
const SHARE_HEIGHT: u32 = 720;
const ARCHIVE_MINUTES: u32 = 60;
const GIF_QUERY: &str = "cat";
const STICKER_PACKS: usize = 3;
const UNICODE_CATEGORIES: usize = 8;
const STICKER_NAME: &str = "headless_sticker";
const STICKER_FILE: &str = "headless.png";
const STICKER_PNG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

pub(super) struct TextOutcome {
	pub(super) threads_created: usize,
	pub(super) groups_created: usize,
	pub(super) moderation_ok: bool,
	pub(super) expressions_ok: bool,
}

impl Harness {
	pub(super) async fn screen_share(&self) -> Result<bool> {
		let target = ScreenSource::list()
			.ok()
			.and_then(|targets| targets.into_iter().next())
			.unwrap_or_else(|| ScreenTarget {
				kind: ScreenTargetKind::Monitor,
				id: 0,
				name: "headless".to_owned(),
				width: SHARE_WIDTH,
				height: SHARE_HEIGHT,
				primary: true,
			});
		self.session.send(Command::StartScreenShare {
			target,
			quality: VideoQuality::default(),
			with_audio: false,
		});
		self.until("the screen share to start", |state| {
			state.voice().is_some_and(|voice| voice.screen_share.is_some())
		})
		.await?;
		self.session.send(Command::StopScreenShare);
		self.until("the screen share to stop", |state| {
			state.voice().is_some_and(|voice| voice.screen_share.is_none())
		})
		.await?;
		Ok(true)
	}

	pub(super) async fn text_features(
		&self,
		fixtures: &Fixtures,
		guild_id: Snowflake,
		channel_id: Snowflake,
		message_id: Snowflake,
	) -> Result<TextOutcome> {
		self.thread_from_message(guild_id, channel_id, message_id).await?;
		self.pin_cycle(channel_id, message_id).await?;
		self.friend_cycle(fixtures).await?;
		self.mute_cycle(guild_id, channel_id).await?;
		let moderation_ok = self.kick_and_audit(guild_id).await?;
		let groups_created = self.group_dm(fixtures).await?;
		let expressions_ok = self.expression_features(guild_id, channel_id).await?;
		Ok(TextOutcome {
			threads_created: 1,
			groups_created,
			moderation_ok,
			expressions_ok,
		})
	}

	async fn thread_from_message(
		&self,
		guild_id: Snowflake,
		channel_id: Snowflake,
		message_id: Snowflake,
	) -> Result<()> {
		self.session.send(Command::CreateThread {
			channel_id,
			message_id: Some(message_id),
			name: THREAD_NAME.to_owned(),
			auto_archive_minutes: ARCHIVE_MINUTES,
		});
		self.until("the thread to appear under its channel", |state| {
			state
				.guild_channel_tree(guild_id)
				.iter()
				.flat_map(|node| node.children.iter())
				.any(|row| {
					row.id == message_id
						&& row.kind == ChannelType::PublicThread
						&& row.member && row.parent_id == Some(channel_id)
				})
		})
		.await?;
		self.session.send(Command::SelectChannel {
			guild_id: Some(guild_id),
			channel_id,
		});
		Ok(())
	}

	async fn pin_cycle(&self, channel_id: Snowflake, message_id: Snowflake) -> Result<()> {
		let pinned = |state: &State| {
			state
				.pins(channel_id)
				.is_some_and(|pins| pins.iter().any(|message| message.id == message_id))
		};
		self.session.send(Command::LoadPins { channel_id });
		self.until("the pin list to load", |state| state.pins(channel_id).is_some())
			.await?;
		self.session.send(Command::PinMessage {
			channel_id,
			message_id,
		});
		self.until("the message to be pinned", pinned).await?;
		self.session.send(Command::UnpinMessage {
			channel_id,
			message_id,
		});
		self.until("the message to be unpinned", |state| !pinned(state))
			.await
	}

	async fn friend_cycle(&self, fixtures: &Fixtures) -> Result<()> {
		let (username, user_id, incoming) = {
			let state = self.state.read();
			let known: Vec<Snowflake> = state
				.friends()
				.into_iter()
				.chain(state.pending_incoming())
				.chain(state.pending_outgoing())
				.chain(state.blocked())
				.map(|row| row.user_id)
				.collect();
			let stranger = fixtures
				.users
				.values()
				.find(|user| user.id.get() != MOCK_USER_ID && !known.contains(&user.id))
				.context("no user left to befriend")?;
			let incoming = state
				.pending_incoming()
				.first()
				.map(|row| row.user_id)
				.context("no incoming friend request in the fixtures")?;
			(stranger.username.clone(), stranger.id, incoming)
		};
		self.session.send(Command::AddFriend { username });
		self.until("the outgoing request to appear", |state| {
			state.pending_outgoing().iter().any(|row| row.user_id == user_id)
		})
		.await?;
		self.session.send(Command::AcceptFriend { user_id: incoming });
		self.until("the incoming request to become a friend", |state| {
			state.friends().iter().any(|row| row.user_id == incoming)
				&& state.pending_incoming().iter().all(|row| row.user_id != incoming)
		})
		.await
	}

	async fn mute_cycle(&self, guild_id: Snowflake, selected: Snowflake) -> Result<()> {
		let row_of = |state: &State, id: Snowflake| {
			state
				.guild_channel_tree(guild_id)
				.into_iter()
				.flat_map(|node| node.children)
				.find(|row| row.id == id)
		};
		let target = self
			.state
			.read()
			.guild_channel_tree(guild_id)
			.into_iter()
			.flat_map(|node| node.children)
			.find(|row| row.unread && !row.muted && row.id != selected)
			.map(|row| row.id)
			.context("no unread channel to mute")?;
		self.session.send(Command::MuteChannel {
			channel_id: target,
			guild_id: Some(guild_id),
			until: None,
		});
		self.until("the channel to be muted and read", |state| {
			row_of(state, target).is_some_and(|row| row.muted && !row.unread)
		})
		.await?;
		self.session.send(Command::UnmuteChannel {
			channel_id: target,
			guild_id: Some(guild_id),
		});
		self.until("the channel to be unmuted and unread", |state| {
			row_of(state, target).is_some_and(|row| !row.muted && row.unread)
		})
		.await
	}

	async fn kick_and_audit(&self, guild_id: Snowflake) -> Result<bool> {
		let member_count = |state: &State| {
			state
				.member_rows(guild_id)
				.iter()
				.filter(|row| matches!(row, MemberListRow::Member(_)))
				.count()
		};
		let (target, before) = {
			let state = self.state.read();
			let me = state.current_user_id();
			let owner = state.guild(guild_id).and_then(|guild| guild.owner_id);
			let target = state
				.member_rows(guild_id)
				.into_iter()
				.find_map(|row| match row {
					MemberListRow::Member(member)
						if Some(member.user_id) != me && Some(member.user_id) != owner =>
					{
						Some(member.user_id)
					}
					_ => None,
				})
				.context("no member to kick")?;
			(target, member_count(&state))
		};
		self.session.send(Command::KickMember {
			guild_id,
			user_id: target,
			reason: Some(KICK_REASON.to_owned()),
		});
		self.until("the member list to shrink", |state| {
			member_count(state) + 1 == before
		})
		.await?;
		self.session.send(Command::LoadAuditLog {
			guild_id,
			before: None,
			action_type: None,
		});
		self.until("the audit log to list the kick", |state| {
			state.audit_log(guild_id).is_some_and(|log| {
				log.entries
					.iter()
					.any(|entry| entry.action_type == AUDIT_KICK && entry.target_id == Some(target))
			})
		})
		.await?;
		Ok(true)
	}

	async fn group_dm(&self, fixtures: &Fixtures) -> Result<usize> {
		let recipients: Vec<Snowflake> = fixtures
			.relationships
			.iter()
			.take(GROUP_MEMBERS)
			.map(|relationship| relationship.id)
			.collect();
		let before = self.state.read().private_channels().count();
		self.session.send(Command::CreateGroupDm {
			recipient_ids: recipients.clone(),
		});
		self.until("the group conversation to appear", |state| {
			state.private_channels().count() == before + 1
				&& state.private_channels().any(|channel| {
					channel.kind == ChannelType::GroupDm
						&& recipients.iter().all(|id| channel.recipient_ids.contains(id))
				})
		})
		.await?;
		Ok(1)
	}

	async fn echoed_message(
		&self,
		what: &str,
		channel_id: Snowflake,
		wanted: impl Fn(&Message) -> bool,
	) -> Result<()> {
		self.until(what, |state| {
			state.messages(channel_id).is_some_and(|cache| {
				cache.pending.is_empty() && cache.ordered.iter().any(|message| wanted(message))
			})
		})
		.await
	}

	async fn expression_features(&self, guild_id: Snowflake, channel_id: Snowflake) -> Result<bool> {
		self.session.send(Command::LoadGifTrending);
		self.until("trending gifs to load", |state| {
			state
				.gif_trending()
				.is_some_and(|trending| !trending.gifs.is_empty())
		})
		.await?;
		self.session.send(Command::SearchGifs {
			query: GIF_QUERY.to_owned(),
		});
		self.until("gif search results", |state| {
			state.gif_search(GIF_QUERY).is_some_and(|gifs| !gifs.is_empty())
		})
		.await?;
		let gif = self
			.state
			.read()
			.gif_search(GIF_QUERY)
			.and_then(|gifs| gifs.first().cloned())
			.context("gif search returned nothing")?;
		self.session.send(Command::SendGif {
			channel_id,
			gif: gif.clone(),
		});
		self.echoed_message("the gif link to be echoed", channel_id, |message| {
			message.content == gif.url
		})
		.await?;
		ensure!(
			self.state.read().recent_gifs().len() == 1,
			"sending a gif did not record exactly one recent gif"
		);
		self.session.send(Command::LoadStickerPacks);
		self.until("sticker packs to load", |state| {
			state
				.sticker_packs()
				.is_some_and(|packs| packs.len() == STICKER_PACKS)
		})
		.await?;
		let sticker_id = self
			.state
			.read()
			.sticker_packs()
			.and_then(|packs| packs.iter().find_map(|pack| pack.stickers.first()))
			.map(|sticker| sticker.id)
			.context("no sticker in the packs")?;
		self.session.send(Command::SendSticker {
			channel_id,
			sticker_id,
		});
		self.echoed_message("the sticker message to be echoed", channel_id, |message| {
			message.sticker_items.iter().any(|item| item.id == sticker_id)
		})
		.await?;
		self.sticker_cycle(guild_id).await?;
		ensure!(
			self.state.read().emoji_catalog(None).unicode.len() == UNICODE_CATEGORIES,
			"the emoji catalog does not list every unicode category"
		);
		Ok(true)
	}

	async fn sticker_cycle(&self, guild_id: Snowflake) -> Result<()> {
		let count = |state: &State| state.guild_stickers(guild_id).map(<[_]>::len);
		self.session.send(Command::LoadGuildStickers { guild_id });
		self.until("server stickers to load", |state| count(state).is_some())
			.await?;
		let before = count(&self.state.read()).unwrap_or_default();
		self.session.send(Command::CreateSticker {
			guild_id,
			name: STICKER_NAME.to_owned(),
			description: String::new(),
			tags: STICKER_NAME.to_owned(),
			file_png_or_apng: STICKER_PNG.to_vec(),
			filename: STICKER_FILE.to_owned(),
		});
		self.until("the new sticker to appear", |state| {
			count(state) == Some(before + 1)
		})
		.await?;
		let created = self
			.state
			.read()
			.guild_stickers(guild_id)
			.and_then(|stickers| stickers.iter().find(|sticker| sticker.name == STICKER_NAME))
			.map(|sticker| sticker.id)
			.context("the created sticker is not listed")?;
		self.session.send(Command::DeleteSticker {
			guild_id,
			sticker_id: created,
		});
		self.until("the sticker to disappear", |state| count(state) == Some(before))
			.await
	}
}
