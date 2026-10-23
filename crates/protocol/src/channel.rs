use bitflags::bitflags;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::lenient::{LenientVec, flags_serde, int_enum, timestamp};
use crate::member::Member;
use crate::permissions::Permissions;
use crate::snowflake::Snowflake;
use crate::user::User;

int_enum! {
	pub enum ChannelType {
		GuildText = 0,
		Dm = 1,
		GuildVoice = 2,
		GroupDm = 3,
		GuildCategory = 4,
		GuildAnnouncement = 5,
		GuildStore = 6,
		AnnouncementThread = 10,
		PublicThread = 11,
		PrivateThread = 12,
		GuildStageVoice = 13,
		GuildDirectory = 14,
		GuildForum = 15,
		GuildMedia = 16,
		Lobby = 17,
		EphemeralDm = 18,
	}
}

int_enum! {
	pub enum PermissionOverwriteKind {
		Role = 0,
		Member = 1,
	}
}

int_enum! {
	pub enum ForumSortOrder {
		LatestActivity = 0,
		CreationDate = 1,
	}
}

int_enum! {
	pub enum ForumLayout {
		NotSet = 0,
		ListView = 1,
		GalleryView = 2,
	}
}

bitflags! {
	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
	pub struct ChannelFlags: u64 {
		const PINNED = 1 << 1;
		const REQUIRE_TAG = 1 << 4;
		const HIDE_MEDIA_DOWNLOAD_OPTIONS = 1 << 15;
	}
}

flags_serde!(ChannelFlags, number);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionOverwrite {
	pub id: Snowflake,
	#[serde(rename = "type")]
	pub kind: PermissionOverwriteKind,
	pub allow: Permissions,
	pub deny: Permissions,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadMetadata {
	pub archived: bool,
	pub auto_archive_duration: Option<i64>,
	#[serde(deserialize_with = "timestamp")]
	pub archive_timestamp: Option<DateTime<Utc>>,
	pub locked: bool,
	pub invitable: Option<bool>,
	#[serde(deserialize_with = "timestamp")]
	pub create_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadMember {
	pub id: Option<Snowflake>,
	pub user_id: Option<Snowflake>,
	pub guild_id: Option<Snowflake>,
	#[serde(deserialize_with = "timestamp")]
	pub join_timestamp: Option<DateTime<Utc>>,
	pub flags: Option<u64>,
	pub member: Option<Member>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ForumTag {
	pub id: Snowflake,
	pub name: String,
	pub moderated: bool,
	pub emoji_id: Option<Snowflake>,
	pub emoji_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DefaultReaction {
	pub emoji_id: Option<Snowflake>,
	pub emoji_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Channel {
	pub id: Snowflake,
	#[serde(rename = "type")]
	pub kind: ChannelType,
	pub guild_id: Option<Snowflake>,
	pub position: Option<i64>,
	pub permission_overwrites: LenientVec<PermissionOverwrite>,
	pub name: Option<String>,
	pub topic: Option<String>,
	pub nsfw: Option<bool>,
	pub last_message_id: Option<Snowflake>,
	pub bitrate: Option<i64>,
	pub user_limit: Option<i64>,
	pub rate_limit_per_user: Option<i64>,
	pub recipients: LenientVec<User>,
	pub recipient_ids: Vec<Snowflake>,
	pub icon: Option<String>,
	pub owner_id: Option<Snowflake>,
	pub parent_id: Option<Snowflake>,
	#[serde(deserialize_with = "timestamp")]
	pub last_pin_timestamp: Option<DateTime<Utc>>,
	pub rtc_region: Option<String>,
	pub message_count: Option<i64>,
	pub member_count: Option<i64>,
	pub thread_metadata: Option<ThreadMetadata>,
	pub member: Option<ThreadMember>,
	pub default_auto_archive_duration: Option<i64>,
	pub flags: ChannelFlags,
	pub available_tags: LenientVec<ForumTag>,
	pub applied_tags: Vec<Snowflake>,
	pub default_reaction_emoji: Option<DefaultReaction>,
	pub default_sort_order: Option<ForumSortOrder>,
	pub default_forum_layout: Option<ForumLayout>,
	pub total_message_sent: Option<i64>,
	pub is_message_request: Option<bool>,
	pub is_spam: Option<bool>,
}
