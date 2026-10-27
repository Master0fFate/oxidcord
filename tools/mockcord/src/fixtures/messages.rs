use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use protocol::{
	Attachment, Channel, Embed, EmbedAuthor, EmbedField, EmbedFooter, EmbedKind, EmbedMedia, EmbedProvider,
	Emoji, LenientVec, Message, MessageFlags, MessageReference, MessageReferenceKind, MessageType,
	PartialEmoji, Poll, PollAnswer, PollAnswerCount, PollLayout, PollMedia, PollResults, Reaction,
	ReactionCountDetails, Role, Snowflake, StickerFormat, StickerItem, User,
};
use rand::Rng;
use rand::rngs::StdRng;
use serde_json::Value;

use super::corpus::{self, pick};
use super::media::thumbhash;
use crate::forward::snapshot_of;
use crate::ids::snowflake_at;
use crate::json::to_json;

const SPAN_MS: i64 = 30 * 24 * 3600 * 1000;
const CLUSTER_SIZE: usize = 4;
const GROUPING_WINDOW_MS: i64 = 300_000;
const RECENT_WINDOW_MS: i64 = 3_600_000;
const STICKER_ID: u64 = 749_054_660_769_218_631;
const ANIMATED_FLAG: u64 = 1 << 5;
const VOICE_WAVEFORM: &str = "AAoUHiw4RlJeanV/iJCWm56foJ+dmpWPh312a19UST4zKB0TCQIACRQgLDhEUFxndHuEjJKXm56foJ+dmpWOhn10aV5TSD0yJxwSCAA=";
const MALFORMED_ATTACHMENT: &str = r#"{"id":{"unexpected":true},"filename":["broken"],"size":"huge"}"#;

pub struct ChannelSpec<'a> {
	pub channel_id: u64,
	pub guild_id: Option<u64>,
	pub count: usize,
	pub specials: bool,
	pub variant: usize,
	pub anchor: DateTime<Utc>,
	pub me: &'a User,
	pub authors: &'a [&'a User],
	pub roles: &'a [Role],
	pub emojis: &'a [Emoji],
	pub channels: &'a [(u64, String)],
	pub threads: &'a [Channel],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Special {
	Formatting,
	CodeBlock,
	Quote,
	Spoiler,
	List,
	Headings,
	Links,
	Mentions,
	CustomEmoji,
	UnicodeEmoji,
	MentionMe,
	Reply,
	Images,
	EmbedRich,
	EmbedImage,
	EmbedVideo,
	EmbedGifv,
	EmbedArticle,
	EmbedLink,
	EmbedSpotify,
	VideoFile,
	WebmFile,
	AudioFile,
	VoiceMessage,
	GifFile,
	GifLink,
	Poll,
	Reactions,
	Pinned,
	PinNotice,
	UserJoin,
	Edited,
	Forward,
	Sticker,
	Unknown,
	Malformed,
	ThreadCreated(usize),
}

pub fn generate(rng: &mut StdRng, spec: &ChannelSpec) -> BTreeMap<u64, Value> {
	let times = timeline(rng, spec.count, spec.anchor.timestamp_millis());
	let mut plan = plan(spec);
	let mut generator = Generator {
		rng,
		spec,
		messages: BTreeMap::new(),
		last_id: 0,
		previous: None,
		previous_author: None,
		last_reply: None,
		last_pinned: None,
	};
	let mut previous_time = i64::MIN / 2;
	for (index, time) in times.into_iter().enumerate() {
		generator.push(time, time - previous_time, plan[index].take());
		previous_time = time;
	}
	generator.messages
}

fn timeline(rng: &mut StdRng, count: usize, anchor_ms: i64) -> Vec<i64> {
	if count == 0 {
		return Vec::new();
	}
	let clusters = (count / CLUSTER_SIZE).max(1);
	let mut starts: Vec<i64> = (0..clusters)
		.map(|_| rng.random_range(0..SPAN_MS - 900_000))
		.collect();
	if let Some(last) = starts.last_mut() {
		*last = SPAN_MS - 1_200_000 - rng.random_range(0..RECENT_WINDOW_MS);
	}
	starts.sort_unstable();
	let origin = anchor_ms - SPAN_MS;
	let mut times: Vec<i64> = Vec::with_capacity(count);
	let mut current_cluster = usize::MAX;
	let mut offset = 0;
	for index in 0..count {
		let cluster = index * clusters / count;
		if cluster == current_cluster {
			offset += rng.random_range(5_000..120_000);
		} else {
			current_cluster = cluster;
			offset = 0;
		}
		let mut time = origin + starts[cluster] + offset;
		if let Some(previous) = times.last() {
			time = time.max(previous + 1_000);
		}
		times.push(time.min(anchor_ms - (count - index) as i64));
	}
	times
}

fn special_list(spec: &ChannelSpec) -> Vec<Special> {
	use Special::*;
	let mut list = vec![
		Formatting,
		CodeBlock,
		Quote,
		Spoiler,
		List,
		Headings,
		Links,
		Mentions,
		CustomEmoji,
		UnicodeEmoji,
		MentionMe,
		Reply,
		Images,
		Reply,
		EmbedRich,
		Poll,
		Reactions,
		Pinned,
		Reply,
		EmbedVideo,
		UserJoin,
		PinNotice,
		EmbedImage,
		Edited,
		Forward,
		Reply,
		EmbedArticle,
		Sticker,
		EmbedGifv,
		Unknown,
		Malformed,
		EmbedLink,
		VideoFile,
		GifLink,
		AudioFile,
		EmbedSpotify,
		VoiceMessage,
		GifFile,
		WebmFile,
		MentionMe,
	];
	if spec.emojis.is_empty() {
		list.retain(|special| *special != CustomEmoji);
	}
	if spec.guild_id.is_none() {
		list.retain(|special| *special != UserJoin);
	}
	for index in 0..spec.threads.len() {
		list.insert((4 + index * 10).min(list.len()), ThreadCreated(index));
	}
	list
}

fn plan(spec: &ChannelSpec) -> Vec<Option<Special>> {
	let mut plan = vec![None; spec.count];
	if !spec.specials || spec.count == 0 {
		return plan;
	}
	let mut specials = special_list(spec);
	let take = specials.len().min(spec.count);
	if take < specials.len() {
		let offset = spec.variant % specials.len();
		specials.rotate_left(offset);
		specials.truncate(take);
	}
	for (slot, special) in specials.into_iter().enumerate() {
		plan[(slot + 1) * spec.count / (take + 1)] = Some(special);
	}
	plan
}

struct Generator<'a, 'r> {
	rng: &'r mut StdRng,
	spec: &'a ChannelSpec<'a>,
	messages: BTreeMap<u64, Value>,
	last_id: u64,
	previous: Option<u64>,
	previous_author: Option<&'a User>,
	last_reply: Option<u64>,
	last_pinned: Option<u64>,
}

impl<'a> Generator<'a, '_> {
	fn push(&mut self, time_ms: i64, gap_ms: i64, special: Option<Special>) {
		let author = self.pick_author(gap_ms);
		let id = snowflake_at(time_ms, 0).max(self.last_id + 1);
		self.last_id = id;
		let timestamp = DateTime::from_timestamp_millis(time_ms).unwrap_or_default();
		let mut message = Message {
			id: Snowflake(id),
			channel_id: Snowflake(self.spec.channel_id),
			author: author.clone(),
			timestamp: Some(timestamp),
			..Message::default()
		};
		match special {
			Some(special) => self.apply_special(&mut message, special, author),
			None => self.apply_ordinary(&mut message, author),
		}
		let mut value = to_json(&message);
		if special == Some(Special::Malformed)
			&& let (Some(attachments), Ok(broken)) = (
				value["attachments"].as_array_mut(),
				serde_json::from_str::<Value>(MALFORMED_ATTACHMENT),
			) {
			attachments.push(broken);
		}
		if message.pinned {
			self.last_pinned = Some(id);
		}
		self.previous = Some(id);
		self.previous_author = Some(author);
		self.messages.insert(id, value);
	}

	fn pick_author(&mut self, gap_ms: i64) -> &'a User {
		let authors = self.spec.authors;
		match self.previous_author {
			Some(previous) if gap_ms < GROUPING_WINDOW_MS && self.rng.random_bool(0.45) => previous,
			_ => authors[self.rng.random_range(0..authors.len())],
		}
	}

	fn other_author(&mut self, author: &User) -> &'a User {
		let authors = self.spec.authors;
		let candidate = authors[self.rng.random_range(0..authors.len())];
		if candidate.id == author.id {
			authors[(self.rng.random_range(0..authors.len()) + 1) % authors.len()]
		} else {
			candidate
		}
	}

	fn apply_ordinary(&mut self, message: &mut Message, author: &User) {
		let roll = self.rng.random_range(0..100);
		message.content = match roll {
			0..35 => pick(self.rng, corpus::SHORT_REPLIES).to_string(),
			35..65 => pick(self.rng, corpus::SENTENCES).to_string(),
			65..72 => format!(
				"{}\n{}",
				pick(self.rng, corpus::SENTENCES),
				pick(self.rng, corpus::SENTENCES)
			),
			72..78 => format!(
				"{} {}",
				pick(self.rng, corpus::SHORT_REPLIES),
				pick(self.rng, corpus::UNICODE_EMOJI)
			),
			78..83 => format!("look at this {}", pick(self.rng, corpus::LINKS)),
			83..88 => {
				let other = self.other_author(author);
				message.mentions = LenientVec::from(vec![other.clone()]);
				format!("<@{}> {}", other.id, pick(self.rng, corpus::SHORT_REPLIES))
			}
			88..91 => format!(
				"try `cargo build` first, {}",
				pick(self.rng, corpus::SHORT_REPLIES)
			),
			91..94 if author.id != self.spec.me.id => {
				message.mentions = LenientVec::from(vec![self.spec.me.clone()]);
				format!("<@{}> {}", self.spec.me.id, pick(self.rng, corpus::SENTENCES))
			}
			_ => pick(self.rng, corpus::SHORT_REPLIES).to_string(),
		};
		if self.rng.random_bool(0.06) {
			let emoji = *pick(self.rng, corpus::UNICODE_EMOJI);
			message.reactions =
				LenientVec::from(vec![reaction(emoji, 1 + self.rng.random_range(0..4), false)]);
		}
	}

	fn apply_special(&mut self, message: &mut Message, special: Special, author: &User) {
		let spec = self.spec;
		let timestamp = message.timestamp.unwrap_or_default();
		match special {
			Special::Formatting => {
				message.content =
					"this is **bold**, this is *italic*, __underlined__, ~~struck through~~ and ***bold italic*** together"
						.to_owned();
			}
			Special::CodeBlock => {
				message.content = "check this out:\n```rust\nfn main() {\n\tlet answer = 42;\n\tprintln!(\"{answer}\");\n}\n```\nruns fine with `cargo run`".to_owned();
			}
			Special::Quote => {
				message.content =
					"> the best code is the code you never have to write\n> - someone wise\nagreed, deleting it now"
						.to_owned();
			}
			Special::Spoiler => {
				message.content =
					"the ending is ||everyone survives|| and the sequel is ||already announced||".to_owned();
			}
			Special::List => {
				message.content =
					"todo before friday:\n- fix the build\n- ship the release\n  - tag it\n  - write notes\n1. first\n2. second"
						.to_owned();
			}
			Special::Headings => {
				message.content =
					"# Release notes\n## Fixes\n### Details\n-# small print goes here".to_owned();
			}
			Special::Links => {
				message.content = format!(
					"docs live at {} and [this masked link](https://example.com/masked) plus <https://example.com/no-embed>",
					pick(self.rng, corpus::LINKS)
				);
			}
			Special::Mentions => self.mentions(message, author),
			Special::CustomEmoji => {
				let still = spec.emojis.iter().find(|emoji| emoji.animated != Some(true));
				let moving = spec.emojis.iter().find(|emoji| emoji.animated == Some(true));
				let mut content = String::from("custom emoji time");
				if let Some(emoji) = still {
					content.push_str(&format!(
						" <:{}:{}>",
						emoji.name.as_deref().unwrap_or("emoji"),
						id_of(emoji)
					));
				}
				if let Some(emoji) = moving {
					content.push_str(&format!(
						" <a:{}:{}>",
						emoji.name.as_deref().unwrap_or("emoji"),
						id_of(emoji)
					));
				}
				if let Some(emoji) = spec.emojis.iter().find(|emoji| emoji.available == Some(false)) {
					let link = format!(
						"https://cdn.discordapp.com/emojis/{}.webp?size=48&name={}",
						id_of(emoji),
						emoji.name.as_deref().unwrap_or("emoji")
					);
					content.push_str(&format!(" {link}"));
					message.embeds = LenientVec::from(vec![Embed {
						kind: Some(EmbedKind::Image),
						url: Some(link.clone()),
						thumbnail: Some(self.media(&link, 48, 48)),
						..Embed::default()
					}]);
				}
				message.content = content;
			}
			Special::UnicodeEmoji => {
				message.content = "ship it 🚀 🔥 ✨ 👍 ❤️ 😂 🎉 🦀 emoji everywhere 👀".to_owned();
			}
			Special::MentionMe => {
				if author.id == spec.me.id {
					message.content = pick(self.rng, corpus::SENTENCES).to_string();
				} else {
					message.content = format!(
						"<@{}> can you take a look at this when you get a minute?",
						spec.me.id
					);
					message.mentions = LenientVec::from(vec![spec.me.clone()]);
				}
			}
			Special::Reply => self.reply(message),
			Special::Images => {
				message.content = "screenshots from today".to_owned();
				let attachments = (0..2)
					.map(|n| self.image_attachment(message.id.0, n))
					.collect::<Vec<_>>();
				message.attachments = LenientVec::from(attachments);
			}
			Special::EmbedRich => {
				message.content = "new release is out".to_owned();
				message.embeds = LenientVec::from(vec![self.rich_embed(timestamp)]);
			}
			Special::EmbedImage => {
				let image = self.media("https://media.discordapp.net/external/pic/photo.png", 1024, 768);
				message.content = "https://media.discordapp.net/external/pic/photo.png".to_owned();
				message.embeds = LenientVec::from(vec![Embed {
					kind: Some(EmbedKind::Image),
					url: Some("https://media.discordapp.net/external/pic/photo.png".to_owned()),
					thumbnail: Some(image),
					..Embed::default()
				}]);
			}
			Special::EmbedVideo => {
				message.content = "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned();
				let thumbnail = self.media("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg", 1280, 720);
				let video = self.media("https://www.youtube.com/embed/dQw4w9WgXcQ", 1280, 720);
				message.embeds = LenientVec::from(vec![Embed {
					kind: Some(EmbedKind::Video),
					title: Some("Never Gonna Give You Up".to_owned()),
					description: Some("The official video".to_owned()),
					url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
					color: Some(0xFF0000),
					provider: Some(EmbedProvider {
						name: Some("YouTube".to_owned()),
						url: Some("https://www.youtube.com".to_owned()),
					}),
					author: Some(EmbedAuthor {
						name: "Rick Astley".to_owned(),
						url: Some("https://www.youtube.com/@RickAstleyYT".to_owned()),
						..EmbedAuthor::default()
					}),
					thumbnail: Some(thumbnail),
					video: Some(video),
					..Embed::default()
				}]);
			}
			Special::EmbedGifv => {
				message.content = "https://tenor.com/view/mock-gif-1".to_owned();
				let thumbnail = self.media("https://media.tenor.com/mock/thumb.png", 498, 280);
				let video = self.media("https://media.tenor.com/mock/gifv.mp4", 240, 136);
				message.embeds = LenientVec::from(vec![Embed {
					kind: Some(EmbedKind::Gifv),
					url: Some("https://tenor.com/view/mock-gif-1".to_owned()),
					provider: Some(EmbedProvider {
						name: Some("Tenor".to_owned()),
						url: Some("https://tenor.com".to_owned()),
					}),
					thumbnail: Some(thumbnail),
					video: Some(video),
					..Embed::default()
				}]);
			}
			Special::EmbedArticle => {
				message.content = "https://example.com/articles/lenient-decoding".to_owned();
				let thumbnail = self.media("https://example.com/static/lenient.png", 600, 315);
				message.embeds = LenientVec::from(vec![Embed {
					kind: Some(EmbedKind::Article),
					title: Some("Why lenient decoding saves your client".to_owned()),
					description: Some(
						"A bad element should never take the whole payload down with it. Here is how to drop one and keep going."
							.to_owned(),
					),
					url: Some("https://example.com/articles/lenient-decoding".to_owned()),
					provider: Some(EmbedProvider {
						name: Some("Example Blog".to_owned()),
						url: None,
					}),
					thumbnail: Some(thumbnail),
					timestamp: Some(timestamp),
					..Embed::default()
				}]);
			}
			Special::EmbedLink => {
				message.content = "https://example.org/plain-link".to_owned();
				message.embeds = LenientVec::from(vec![Embed {
					kind: Some(EmbedKind::Link),
					title: Some("A plain link preview".to_owned()),
					url: Some("https://example.org/plain-link".to_owned()),
					description: Some("No image, no author, just a title and a line of text.".to_owned()),
					..Embed::default()
				}]);
			}
			Special::EmbedSpotify => {
				let link = "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT";
				message.content = link.to_owned();
				message.embeds = LenientVec::from(vec![Embed {
					kind: Some(EmbedKind::Link),
					title: Some("Never Gonna Give You Up".to_owned()),
					description: Some("Rick Astley · Song · 1987".to_owned()),
					url: Some(link.to_owned()),
					provider: Some(EmbedProvider {
						name: Some("Spotify".to_owned()),
						url: Some("https://www.spotify.com/".to_owned()),
					}),
					thumbnail: Some(self.media("https://i.scdn.co/image/mock-cover.jpg", 640, 640)),
					..Embed::default()
				}]);
			}
			Special::VideoFile => {
				message.content = "clip from today".to_owned();
				message.attachments = LenientVec::from(vec![self.file_attachment(
					message.id.0,
					"clip.mp4",
					"video/mp4",
					Some((320, 180)),
				)]);
			}
			Special::WebmFile => {
				message.attachments = LenientVec::from(vec![self.file_attachment(
					message.id.0,
					"clip.webm",
					"video/webm",
					Some((320, 180)),
				)]);
			}
			Special::AudioFile => {
				message.content = "new track".to_owned();
				message.attachments = LenientVec::from(vec![self.file_attachment(
					message.id.0,
					"song.mp3",
					"audio/mpeg",
					None,
				)]);
			}
			Special::VoiceMessage => {
				let mut voice = self.file_attachment(message.id.0, "voice.ogg", "audio/ogg", None);
				voice.duration_secs = Some(3.0);
				voice.waveform = Some(VOICE_WAVEFORM.to_owned());
				message.flags |= MessageFlags::IS_VOICE_MESSAGE;
				message.attachments = LenientVec::from(vec![voice]);
			}
			Special::GifFile => {
				let mut gif = self.file_attachment(message.id.0, "loop.gif", "image/gif", Some((200, 120)));
				gif.flags = Some(ANIMATED_FLAG);
				message.attachments = LenientVec::from(vec![gif]);
			}
			Special::GifLink => {
				let link = "https://example.com/reactions/loop.gif";
				message.content = link.to_owned();
				let mut image = self.media(link, 200, 120);
				image.content_type = Some("image/gif".to_owned());
				image.flags = Some(ANIMATED_FLAG);
				message.embeds = LenientVec::from(vec![Embed {
					kind: Some(EmbedKind::Image),
					url: Some(link.to_owned()),
					thumbnail: Some(image),
					..Embed::default()
				}]);
			}
			Special::Poll => {
				message.poll = Some(self.poll());
			}
			Special::Reactions => {
				message.content = "react if you agree".to_owned();
				let mut reactions = vec![
					reaction("👍", 12, true),
					reaction("🔥", 4, false),
					reaction("😂", 1, false),
					reaction("🎉", 3, true),
				];
				if let Some(emoji) = spec.emojis.first() {
					reactions.push(Reaction {
						count: 7,
						count_details: Some(ReactionCountDetails { burst: 0, normal: 7 }),
						me: true,
						me_burst: false,
						emoji: PartialEmoji {
							id: emoji.id,
							name: emoji.name.clone(),
							animated: emoji.animated.unwrap_or(false),
						},
						burst_colors: Vec::new(),
					});
				}
				message.reactions = LenientVec::from(reactions);
			}
			Special::Pinned => {
				message.content = "pinned: be kind, keep it on topic, and have fun".to_owned();
				message.pinned = true;
			}
			Special::PinNotice => self.pin_notice(message),
			Special::UserJoin => {
				message.kind = MessageType::UserJoin;
			}
			Special::Edited => {
				message.content = "this message was edited after the fact".to_owned();
				message.edited_timestamp = Some(timestamp + Duration::minutes(10));
			}
			Special::Forward => self.forward(message),
			Special::Sticker => {
				message.sticker_items = LenientVec::from(vec![StickerItem {
					id: Snowflake(STICKER_ID),
					name: "Wave".to_owned(),
					format_type: Some(StickerFormat::Png),
				}]);
			}
			Special::Unknown => {
				message.kind = MessageType::Unknown(999);
				message.content = "a message type this client has never heard of".to_owned();
			}
			Special::Malformed => {
				message.content = "one attachment on this message is broken on purpose".to_owned();
				message.attachments = LenientVec::from(vec![self.image_attachment(message.id.0, 0)]);
			}
			Special::ThreadCreated(index) => {
				let thread = &spec.threads[index];
				message.kind = MessageType::ThreadCreated;
				message.content = thread.name.clone().unwrap_or_default();
				message.message_reference = Some(MessageReference {
					kind: None,
					message_id: None,
					channel_id: Some(thread.id),
					guild_id: spec.guild_id.map(Snowflake),
					fail_if_not_exists: None,
				});
			}
		}
	}

	fn mentions(&mut self, message: &mut Message, author: &User) {
		let spec = self.spec;
		let other = self.other_author(author);
		let mut content = format!("<@{}>", other.id);
		message.mentions = LenientVec::from(vec![other.clone()]);
		if let Some(role) = spec.roles.get(2) {
			content.push_str(&format!(" and <@&{}>", role.id));
			message.mention_roles = vec![role.id];
		}
		if let Some((channel_id, _)) = spec.channels.first() {
			content.push_str(&format!(" please check <#{channel_id}>"));
		}
		message.content = content;
	}

	fn reply(&mut self, message: &mut Message) {
		message.content = pick(self.rng, corpus::SHORT_REPLIES).to_string();
		let target = match self.last_reply {
			Some(previous) if self.rng.random_bool(0.5) => Some(previous),
			_ => self.previous,
		};
		let Some(target) = target else {
			return;
		};
		message.kind = MessageType::Reply;
		message.message_reference = Some(self.reference(MessageReferenceKind::Default, target));
		message.referenced_message = self.stored(target).map(Box::new);
		self.last_reply = Some(message.id.0);
	}

	fn pin_notice(&mut self, message: &mut Message) {
		let Some(pinned) = self.last_pinned else {
			message.content = pick(self.rng, corpus::SHORT_REPLIES).to_string();
			return;
		};
		message.kind = MessageType::ChannelPinnedMessage;
		message.message_reference = Some(self.reference(MessageReferenceKind::Default, pinned));
	}

	fn forward(&mut self, message: &mut Message) {
		let Some(source) = self.previous.and_then(|id| self.stored(id)) else {
			message.content = pick(self.rng, corpus::SHORT_REPLIES).to_string();
			return;
		};
		message.message_reference = Some(self.reference(MessageReferenceKind::Forward, source.id.0));
		message.flags = MessageFlags::HAS_SNAPSHOT;
		message.message_snapshots = LenientVec::from(vec![snapshot_of(source)]);
	}

	fn reference(&self, kind: MessageReferenceKind, message_id: u64) -> MessageReference {
		MessageReference {
			kind: Some(kind),
			message_id: Some(Snowflake(message_id)),
			channel_id: Some(Snowflake(self.spec.channel_id)),
			guild_id: self.spec.guild_id.map(Snowflake),
			fail_if_not_exists: None,
		}
	}

	fn stored(&self, id: u64) -> Option<Message> {
		let mut value = self.messages.get(&id)?.clone();
		value["referenced_message"] = Value::Null;
		serde_json::from_value(value).ok()
	}

	fn image_attachment(&mut self, message_id: u64, index: u64) -> Attachment {
		let (width, height) = *pick(
			self.rng,
			&[(1280, 720), (800, 1200), (1920, 1080), (640, 640), (1024, 768)],
		);
		let attachment_id = message_id + 1 + index;
		let url = format!(
			"https://media.discordapp.net/attachments/{}/{attachment_id}/img{index}.png",
			self.spec.channel_id
		);
		Attachment {
			id: Snowflake(attachment_id),
			filename: format!("img{index}.png"),
			content_type: Some("image/png".to_owned()),
			size: Some(width * height / 8),
			proxy_url: Some(url.clone()),
			url: Some(url),
			width: Some(width),
			height: Some(height),
			flags: Some(0),
			placeholder: Some(thumbhash(self.rng, width >= height)),
			..Attachment::default()
		}
	}

	fn file_attachment(
		&mut self,
		message_id: u64,
		file: &str,
		content_type: &str,
		size: Option<(i64, i64)>,
	) -> Attachment {
		let attachment_id = message_id + 1;
		let url = format!(
			"https://cdn.discordapp.com/attachments/{}/{attachment_id}/{file}",
			self.spec.channel_id
		);
		Attachment {
			id: Snowflake(attachment_id),
			filename: file.to_owned(),
			content_type: Some(content_type.to_owned()),
			size: Some(self.rng.random_range(10_000..200_000)),
			proxy_url: Some(url.replace("cdn.discordapp.com", "media.discordapp.net")),
			url: Some(url),
			width: size.map(|(width, _)| width),
			height: size.map(|(_, height)| height),
			flags: Some(0),
			placeholder: size.map(|(width, height)| thumbhash(self.rng, width >= height)),
			..Attachment::default()
		}
	}

	fn media(&mut self, url: &str, width: i64, height: i64) -> EmbedMedia {
		EmbedMedia {
			url: Some(url.to_owned()),
			proxy_url: Some(url.replace(
				"https://",
				"https://images-ext.discordapp.net/external/mock/https/",
			)),
			width: Some(width),
			height: Some(height),
			content_type: None,
			placeholder: Some(thumbhash(self.rng, width >= height)),
			flags: None,
		}
	}

	fn rich_embed(&mut self, timestamp: DateTime<Utc>) -> Embed {
		let image = self.media(
			"https://media.discordapp.net/attachments/1/2/banner.png",
			1200,
			600,
		);
		let thumbnail = self.media("https://media.discordapp.net/attachments/1/2/logo.png", 256, 256);
		Embed {
			kind: Some(EmbedKind::Rich),
			title: Some("Release 2.4.0".to_owned()),
			description: Some("Faster startup, fewer crashes, and a **brand new** settings page.".to_owned()),
			url: Some("https://example.com/releases/2.4.0".to_owned()),
			timestamp: Some(timestamp),
			color: Some(0x6666ff),
			footer: Some(EmbedFooter {
				text: "oxid release bot".to_owned(),
				icon_url: Some("https://media.discordapp.net/attachments/1/2/bot.png".to_owned()),
				proxy_icon_url: None,
			}),
			author: Some(EmbedAuthor {
				name: "Release Bot".to_owned(),
				url: Some("https://example.com".to_owned()),
				icon_url: Some("https://media.discordapp.net/attachments/1/2/bot.png".to_owned()),
				proxy_icon_url: None,
			}),
			image: Some(image),
			thumbnail: Some(thumbnail),
			fields: LenientVec::from(vec![
				EmbedField {
					name: "Version".to_owned(),
					value: "2.4.0".to_owned(),
					inline: true,
				},
				EmbedField {
					name: "Channel".to_owned(),
					value: "stable".to_owned(),
					inline: true,
				},
				EmbedField {
					name: "Changelog".to_owned(),
					value: "- startup is 40% faster\n- fixed the flicker\n- new settings page".to_owned(),
					inline: false,
				},
			]),
			..Embed::default()
		}
	}

	fn poll(&mut self) -> Poll {
		let answers = [("Rust", "🦀"), ("Zig", "⚡"), ("Go", "🐹")];
		Poll {
			question: PollMedia {
				text: Some("What should we write the next tool in?".to_owned()),
				emoji: None,
			},
			answers: LenientVec::from(
				answers
					.iter()
					.enumerate()
					.map(|(index, (text, emoji))| PollAnswer {
						answer_id: Some(index as i64 + 1),
						poll_media: PollMedia {
							text: Some((*text).to_owned()),
							emoji: Some(PartialEmoji {
								id: None,
								name: Some((*emoji).to_owned()),
								animated: false,
							}),
						},
					})
					.collect::<Vec<_>>(),
			),
			expiry: Some(self.spec.anchor + Duration::days(2)),
			allow_multiselect: false,
			layout_type: Some(PollLayout::Default),
			results: Some(PollResults {
				is_finalized: false,
				answer_counts: LenientVec::from(vec![
					PollAnswerCount {
						id: 1,
						count: 12,
						me_voted: true,
					},
					PollAnswerCount {
						id: 2,
						count: 3,
						me_voted: false,
					},
					PollAnswerCount {
						id: 3,
						count: 1,
						me_voted: false,
					},
				]),
			}),
		}
	}
}

fn reaction(name: &str, count: i64, me: bool) -> Reaction {
	Reaction {
		count,
		count_details: Some(ReactionCountDetails {
			burst: 0,
			normal: count,
		}),
		me,
		me_burst: false,
		emoji: PartialEmoji {
			id: None,
			name: Some(name.to_owned()),
			animated: false,
		},
		burst_colors: Vec::new(),
	}
}

fn id_of(emoji: &Emoji) -> u64 {
	emoji.id.map(Snowflake::get).unwrap_or_default()
}
