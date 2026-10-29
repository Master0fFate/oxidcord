use std::fmt;

use bitflags::bitflags;
use chrono::{DateTime, Utc};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::channel::{Channel, ChannelType};
use crate::component::Component;
use crate::emoji::{PartialEmoji, StickerItem};
use crate::lenient::{LenientVec, flags_serde, int_enum, str_enum, timestamp};
use crate::member::Member;
use crate::snowflake::Snowflake;
use crate::user::User;

int_enum! {
	pub enum MessageType {
		Default = 0,
		RecipientAdd = 1,
		RecipientRemove = 2,
		Call = 3,
		ChannelNameChange = 4,
		ChannelIconChange = 5,
		ChannelPinnedMessage = 6,
		UserJoin = 7,
		PremiumGuildSubscription = 8,
		PremiumGuildSubscriptionTier1 = 9,
		PremiumGuildSubscriptionTier2 = 10,
		PremiumGuildSubscriptionTier3 = 11,
		ChannelFollowAdd = 12,
		ThreadCreated = 18,
		Reply = 19,
		ChatInputCommand = 20,
		ThreadStarterMessage = 21,
		GuildInviteReminder = 22,
		ContextMenuCommand = 23,
		AutoModerationAction = 24,
		PollResult = 46,
	}
}

int_enum! {
	pub enum MessageReferenceKind {
		Default = 0,
		Forward = 1,
	}
}

int_enum! {
	pub enum PollLayout {
		Default = 1,
	}
}

str_enum! {
	pub enum EmbedKind {
		Rich = "rich",
		Image = "image",
		Video = "video",
		Gifv = "gifv",
		Article = "article",
		Link = "link",
		PollResult = "poll_result",
		Components = "components",
	}
}

bitflags! {
	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
	pub struct MessageFlags: u64 {
		const CROSSPOSTED = 1 << 0;
		const IS_CROSSPOST = 1 << 1;
		const SUPPRESS_EMBEDS = 1 << 2;
		const SOURCE_MESSAGE_DELETED = 1 << 3;
		const URGENT = 1 << 4;
		const HAS_THREAD = 1 << 5;
		const EPHEMERAL = 1 << 6;
		const LOADING = 1 << 7;
		const FAILED_TO_MENTION_SOME_ROLES_IN_THREAD = 1 << 8;
		const SUPPRESS_NOTIFICATIONS = 1 << 12;
		const IS_VOICE_MESSAGE = 1 << 13;
		const HAS_SNAPSHOT = 1 << 14;
		const IS_COMPONENTS_V2 = 1 << 15;
	}
}

flags_serde!(MessageFlags, number);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Nonce(pub String);

impl Serialize for Nonce {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.0)
	}
}

struct NonceVisitor;

impl Visitor<'_> for NonceVisitor {
	type Value = Nonce;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a string or an integer")
	}

	fn visit_str<E: de::Error>(self, value: &str) -> Result<Nonce, E> {
		Ok(Nonce(value.to_owned()))
	}

	fn visit_u64<E: de::Error>(self, value: u64) -> Result<Nonce, E> {
		Ok(Nonce(value.to_string()))
	}

	fn visit_i64<E: de::Error>(self, value: i64) -> Result<Nonce, E> {
		Ok(Nonce(value.to_string()))
	}
}

impl<'de> Deserialize<'de> for Nonce {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_any(NonceVisitor)
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Attachment {
	pub id: Snowflake,
	pub filename: String,
	pub title: Option<String>,
	pub description: Option<String>,
	pub content_type: Option<String>,
	pub size: Option<i64>,
	pub url: Option<String>,
	pub proxy_url: Option<String>,
	pub height: Option<i64>,
	pub width: Option<i64>,
	pub ephemeral: Option<bool>,
	pub duration_secs: Option<f64>,
	pub waveform: Option<String>,
	pub flags: Option<u64>,
	pub placeholder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbedFooter {
	pub text: String,
	pub icon_url: Option<String>,
	pub proxy_icon_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbedMedia {
	pub url: Option<String>,
	pub proxy_url: Option<String>,
	pub height: Option<i64>,
	pub width: Option<i64>,
	pub content_type: Option<String>,
	pub placeholder: Option<String>,
	pub flags: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbedProvider {
	pub name: Option<String>,
	pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbedAuthor {
	pub name: String,
	pub url: Option<String>,
	pub icon_url: Option<String>,
	pub proxy_icon_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbedField {
	pub name: String,
	pub value: String,
	pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Embed {
	pub title: Option<String>,
	#[serde(rename = "type")]
	pub kind: Option<EmbedKind>,
	pub description: Option<String>,
	pub url: Option<String>,
	#[serde(deserialize_with = "timestamp")]
	pub timestamp: Option<DateTime<Utc>>,
	pub color: Option<i64>,
	pub footer: Option<EmbedFooter>,
	pub image: Option<EmbedMedia>,
	pub thumbnail: Option<EmbedMedia>,
	pub video: Option<EmbedMedia>,
	pub provider: Option<EmbedProvider>,
	pub author: Option<EmbedAuthor>,
	pub fields: LenientVec<EmbedField>,
	pub components: LenientVec<Component>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReactionCountDetails {
	pub burst: i64,
	pub normal: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Reaction {
	pub count: i64,
	pub count_details: Option<ReactionCountDetails>,
	pub me: bool,
	pub me_burst: bool,
	pub emoji: PartialEmoji,
	pub burst_colors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelMention {
	pub id: Snowflake,
	pub guild_id: Option<Snowflake>,
	#[serde(rename = "type")]
	pub kind: ChannelType,
	pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageActivity {
	#[serde(rename = "type")]
	pub kind: Option<i64>,
	pub party_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageApplication {
	pub id: Snowflake,
	pub name: Option<String>,
	pub icon: Option<String>,
	pub description: Option<String>,
	pub cover_image: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageReference {
	#[serde(rename = "type", skip_serializing_if = "Option::is_none")]
	pub kind: Option<MessageReferenceKind>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub message_id: Option<Snowflake>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub channel_id: Option<Snowflake>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub guild_id: Option<Snowflake>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub fail_if_not_exists: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PollMedia {
	pub text: Option<String>,
	pub emoji: Option<PartialEmoji>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PollAnswer {
	pub answer_id: Option<i64>,
	pub poll_media: PollMedia,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PollAnswerCount {
	pub id: i64,
	pub count: i64,
	pub me_voted: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PollResults {
	pub is_finalized: bool,
	pub answer_counts: LenientVec<PollAnswerCount>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Poll {
	pub question: PollMedia,
	pub answers: LenientVec<PollAnswer>,
	#[serde(deserialize_with = "timestamp")]
	pub expiry: Option<DateTime<Utc>>,
	pub allow_multiselect: bool,
	pub layout_type: Option<PollLayout>,
	pub results: Option<PollResults>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CallInfo {
	pub participants: Vec<Snowflake>,
	#[serde(deserialize_with = "timestamp")]
	pub ended_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PartialMessage {
	#[serde(rename = "type")]
	pub kind: MessageType,
	pub content: String,
	#[serde(deserialize_with = "timestamp")]
	pub timestamp: Option<DateTime<Utc>>,
	#[serde(deserialize_with = "timestamp")]
	pub edited_timestamp: Option<DateTime<Utc>>,
	pub mentions: LenientVec<User>,
	pub mention_roles: Vec<Snowflake>,
	pub attachments: LenientVec<Attachment>,
	pub embeds: LenientVec<Embed>,
	pub sticker_items: LenientVec<StickerItem>,
	pub flags: MessageFlags,
	pub components: LenientVec<Component>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageSnapshot {
	pub message: PartialMessage,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Message {
	pub id: Snowflake,
	pub channel_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub author: User,
	pub member: Option<Member>,
	pub content: String,
	#[serde(deserialize_with = "timestamp")]
	pub timestamp: Option<DateTime<Utc>>,
	#[serde(deserialize_with = "timestamp")]
	pub edited_timestamp: Option<DateTime<Utc>>,
	pub tts: bool,
	pub mention_everyone: bool,
	pub mentions: LenientVec<User>,
	pub mention_roles: Vec<Snowflake>,
	pub mention_channels: LenientVec<ChannelMention>,
	pub attachments: LenientVec<Attachment>,
	pub embeds: LenientVec<Embed>,
	pub reactions: LenientVec<Reaction>,
	pub nonce: Option<Nonce>,
	pub pinned: bool,
	pub webhook_id: Option<Snowflake>,
	#[serde(rename = "type")]
	pub kind: MessageType,
	pub activity: Option<MessageActivity>,
	pub application: Option<MessageApplication>,
	pub application_id: Option<Snowflake>,
	pub flags: MessageFlags,
	pub message_reference: Option<MessageReference>,
	pub referenced_message: Option<Box<Message>>,
	pub message_snapshots: LenientVec<MessageSnapshot>,
	pub interaction_metadata: Value,
	pub thread: Option<Box<Channel>>,
	pub components: LenientVec<Component>,
	pub sticker_items: LenientVec<StickerItem>,
	pub position: Option<i64>,
	pub poll: Option<Poll>,
	pub call: Option<CallInfo>,
}

impl Message {
	pub fn is_forward(&self) -> bool {
		self.message_reference
			.as_ref()
			.is_some_and(|reference| reference.kind == Some(MessageReferenceKind::Forward))
			|| !self.message_snapshots.is_empty()
	}

	pub fn forwarded_message(&self) -> Option<&PartialMessage> {
		self.message_snapshots.first().map(|snapshot| &snapshot.message)
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageUpdate {
	pub id: Snowflake,
	pub channel_id: Snowflake,
	pub guild_id: Option<Snowflake>,
	pub author: Option<User>,
	pub member: Option<Member>,
	pub content: Option<String>,
	#[serde(deserialize_with = "timestamp")]
	pub timestamp: Option<DateTime<Utc>>,
	#[serde(deserialize_with = "timestamp")]
	pub edited_timestamp: Option<DateTime<Utc>>,
	pub tts: Option<bool>,
	pub mention_everyone: Option<bool>,
	pub mentions: Option<LenientVec<User>>,
	pub mention_roles: Option<Vec<Snowflake>>,
	pub mention_channels: Option<LenientVec<ChannelMention>>,
	pub attachments: Option<LenientVec<Attachment>>,
	pub embeds: Option<LenientVec<Embed>>,
	pub reactions: Option<LenientVec<Reaction>>,
	pub nonce: Option<Nonce>,
	pub pinned: Option<bool>,
	pub webhook_id: Option<Snowflake>,
	#[serde(rename = "type")]
	pub kind: Option<MessageType>,
	pub flags: Option<MessageFlags>,
	pub message_reference: Option<MessageReference>,
	pub referenced_message: Option<Box<Message>>,
	pub components: Option<LenientVec<Component>>,
	pub sticker_items: Option<LenientVec<StickerItem>>,
	pub poll: Option<Poll>,
}
