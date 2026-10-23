use std::cmp::Reverse;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use protocol::{Channel, ChannelFlags, ForumSortOrder, Message, Snowflake, ThreadMember};

use crate::changes::Changes;
use crate::guild::is_thread;
use crate::state::State;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForumPost {
	pub id: Snowflake,
	pub name: String,
	pub owner_id: Option<Snowflake>,
	pub tags: Vec<Snowflake>,
	pub message_count: i64,
	pub last_activity: DateTime<Utc>,
	pub created_at: DateTime<Utc>,
	pub archived: bool,
	pub locked: bool,
	pub pinned: bool,
	pub member: bool,
}

fn last_activity(thread: &Channel) -> DateTime<Utc> {
	thread
		.last_message_id
		.map(Snowflake::timestamp)
		.or_else(|| {
			thread
				.thread_metadata
				.as_ref()
				.and_then(|metadata| metadata.create_timestamp)
		})
		.unwrap_or_else(|| thread.id.timestamp())
}

fn forum_post(thread: &Channel) -> ForumPost {
	let metadata = thread.thread_metadata.as_ref();
	ForumPost {
		id: thread.id,
		name: thread.name.clone().unwrap_or_default(),
		owner_id: thread.owner_id,
		tags: thread.applied_tags.clone(),
		message_count: thread.message_count.unwrap_or(0),
		last_activity: last_activity(thread),
		created_at: thread.id.timestamp(),
		archived: metadata.is_some_and(|metadata| metadata.archived),
		locked: metadata.is_some_and(|metadata| metadata.locked),
		pinned: thread.flags.contains(ChannelFlags::PINNED),
		member: thread.member.is_some(),
	}
}

impl State {
	pub fn forum_starter(&self, post_id: Snowflake) -> Option<&Arc<Message>> {
		let cache = self.messages.get(&post_id)?;
		cache.by_id.get(&post_id).or_else(|| cache.ordered.first())
	}

	pub fn forum_posts(&self, channel_id: Snowflake) -> Vec<ForumPost> {
		let Some(guild_id) = self.channel_guild(channel_id) else {
			return Vec::new();
		};
		let Some(guild) = self.guilds.get(&guild_id) else {
			return Vec::new();
		};
		let by_creation = guild
			.channels
			.get(&channel_id)
			.is_some_and(|forum| forum.default_sort_order == Some(ForumSortOrder::CreationDate));
		let mut posts: Vec<ForumPost> = guild
			.threads
			.values()
			.filter(|thread| thread.parent_id == Some(channel_id))
			.map(forum_post)
			.collect();
		posts.sort_by_key(|post| {
			let ordering = if by_creation {
				post.created_at
			} else {
				post.last_activity
			};
			(Reverse(post.pinned), Reverse(ordering), Reverse(post.id))
		});
		posts
	}

	pub(crate) fn store_threads(
		&mut self,
		guild_id: Option<Snowflake>,
		threads: &[Channel],
		members: &[ThreadMember],
		changes: &mut Changes,
	) -> Vec<Snowflake> {
		let mut ids = Vec::with_capacity(threads.len());
		for thread in threads.iter().filter(|thread| is_thread(thread.kind)) {
			let Some(guild_id) = thread
				.guild_id
				.or(guild_id)
				.or_else(|| thread.parent_id.and_then(|parent| self.channel_guild(parent)))
			else {
				continue;
			};
			let Some(guild) = self.guilds.get_mut(&guild_id) else {
				continue;
			};
			let mut stored = thread.clone();
			stored.guild_id = Some(guild_id);
			if stored.member.is_none() {
				stored.member = guild.threads.get(&stored.id).and_then(|old| old.member.clone());
			}
			ids.push(stored.id);
			self.channel_guilds.insert(stored.id, Some(guild_id));
			guild.insert_channel(stored);
			changes.channel_lists.insert(guild_id);
			changes.guild_list = true;
			if let Some(parent_id) = thread.parent_id {
				changes.loaded.insert(parent_id);
			}
		}
		for member in members {
			self.set_thread_member(member, changes);
		}
		ids
	}

	pub(crate) fn set_thread_member(&mut self, member: &ThreadMember, changes: &mut Changes) {
		let Some(thread_id) = member.id else {
			return;
		};
		let me = self.current_user_id();
		if member.user_id.is_some() && member.user_id != me {
			return;
		}
		let Some(thread) = self.channel_mut(thread_id) else {
			return;
		};
		thread.member = Some(member.clone());
		self.mark_channel_dirty(thread_id, changes);
	}

	pub(crate) fn thread_members_update(&mut self, update: &protocol::ThreadMembersUpdate) -> Changes {
		let mut changes = Changes::default();
		let me = self.current_user_id();
		let joined = update
			.added_members
			.iter()
			.find(|member| member.user_id.is_none() || member.user_id == me)
			.cloned();
		let left = me.is_some_and(|me| update.removed_member_ids.contains(&me));
		let Some(thread) = self.channel_mut(update.id) else {
			return changes;
		};
		if let Some(count) = update.member_count {
			thread.member_count = Some(count);
		}
		if let Some(mut member) = joined {
			member.id = Some(update.id);
			thread.member = Some(member);
		} else if left {
			thread.member = None;
		}
		self.mark_channel_dirty(update.id, &mut changes);
		changes
	}
}
