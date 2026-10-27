use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use protocol::{ChannelType, Message, Snowflake, StatusKind};
use smallvec::SmallVec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildSummary {
	pub id: Snowflake,
	pub name: String,
	pub icon: Option<String>,
	pub unread: bool,
	pub mentions: u32,
	pub muted: bool,
	pub folder: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRow {
	pub id: Snowflake,
	pub kind: ChannelType,
	pub name: String,
	pub parent_id: Option<Snowflake>,
	pub position: i64,
	pub unread: bool,
	pub mentions: u32,
	pub muted: bool,
	pub locked: bool,
	pub member: bool,
	pub voice_members: Vec<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelTreeNode {
	pub category: Option<ChannelRow>,
	pub children: Vec<ChannelRow>,
	pub collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMessage {
	pub ast: Vec<markdown::Node>,
	pub hidden: bool,
	pub deleted: bool,
	pub revision: u64,
}

impl RenderedMessage {
	pub fn plain(content: &str, hidden: bool, revision: u64) -> Self {
		Self {
			ast: markdown::parse(content).nodes,
			hidden,
			deleted: false,
			revision,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserOverride {
	pub name: Option<String>,
	pub color: Option<u32>,
	pub badge: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MessageView {
	pub message: Arc<Message>,
	pub grouped_with_previous: bool,
	pub pending: bool,
	pub failed: bool,
	pub failure_reason: Option<String>,
	pub plugin: Option<String>,
	pub altered_by: Option<String>,
	pub rendered: Option<Arc<RenderedMessage>>,
	pub deleted: bool,
	pub author_color: Option<u32>,
	pub author_display: String,
	pub author_badge: Option<String>,
	pub member_roles: SmallVec<[Snowflake; 4]>,
	pub mentions_me: bool,
}

#[derive(Debug, Clone)]
pub enum MessageRow {
	Message(MessageView),
	DaySeparator(NaiveDate),
	UnreadMarker,
	HistoryStart,
	Loading,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
	pub user_id: Snowflake,
	pub display: String,
	pub color: Option<u32>,
	pub badge: Option<String>,
	pub status: StatusKind,
	pub hoisted_role: Option<Snowflake>,
	pub activity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberListRow {
	Group { id: String, name: String, count: u32 },
	Member(MemberRow),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmRow {
	pub channel_id: Snowflake,
	pub name: String,
	pub icon: Option<String>,
	pub recipients: Vec<Snowflake>,
	pub unread: bool,
	pub mentions: u32,
	pub last_message_id: Option<Snowflake>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationshipRow {
	pub user_id: Snowflake,
	pub username: String,
	pub display: String,
	pub avatar: Option<String>,
	pub nickname: Option<String>,
	pub status: StatusKind,
	pub activity: Option<String>,
	pub since: Option<DateTime<Utc>>,
}
