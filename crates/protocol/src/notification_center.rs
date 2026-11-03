use serde::{Deserialize, Serialize};

use crate::lenient::{LenientVec, str_enum};
use crate::message::Message;
use crate::snowflake::Snowflake;
use crate::user::User;

str_enum! {
	pub enum NotificationItemKind {
		GoLivePush = "go_live_push",
		FriendRequestAccepted = "friend_request_accepted",
		FriendRequestPending = "friend_request_pending",
		FriendSuggestionCreated = "friend_suggestion_created",
		FriendRequestReminder = "friend_request_reminder",
		DmFriendNudge = "dm_friend_nudge",
		RecentMention = "recent_mention",
		ReplyMention = "reply_mention",
		ScheduledGuildEventStarted = "scheduled_guild_event_started",
		MissedMessages = "missed_messages",
		LifecycleItem = "lifecycle_item",
		GameFriendRequestAccepted = "game_friend_request_accepted",
		ReactionSent = "reaction_sent",
	}
}

impl NotificationItemKind {
	pub fn is_mention(&self) -> bool {
		matches!(self, Self::RecentMention | Self::ReplyMention)
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationCenterItem {
	pub id: Snowflake,
	pub bundle_id: String,
	#[serde(rename = "type")]
	pub kind: NotificationItemKind,
	pub body: String,
	pub acked: bool,
	pub icon_url: Option<String>,
	pub other_user: Option<User>,
	pub message: Option<Box<Message>>,
	pub guild_id: Option<Snowflake>,
	pub message_id: Option<Snowflake>,
	pub message_channel_id: Option<Snowflake>,
	pub callout: Option<String>,
	pub emoji_name: Option<String>,
	pub message_content: Option<String>,
	pub disable_action: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationCenterPage {
	pub items: LenientVec<NotificationCenterItem>,
	pub cursor: Option<Snowflake>,
	pub has_more: bool,
}
