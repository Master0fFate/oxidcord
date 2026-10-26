use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use protocol::lenient::timestamp;
use protocol::{
	Channel, Gif, GifFavorites, GifTrending, LenientVec, Member, Message, Snowflake, Sticker, StickerPack,
	ThreadMember, User,
};
use serde::Deserialize;
use serde_json::Value;

use crate::changes::Changes;
use crate::expressions::{EXPRESSIONS_KEY, EmojiRef, Expressions};
use crate::state::{State, intern_user};

#[derive(Debug, Clone)]
pub enum Loaded {
	Pins {
		channel_id: Snowflake,
		messages: Vec<Message>,
	},
	ArchivedThreads {
		channel_id: Snowflake,
		threads: Vec<Channel>,
		members: Vec<ThreadMember>,
		has_more: bool,
		append: bool,
	},
	ActiveThreads {
		guild_id: Snowflake,
		threads: Vec<Channel>,
		members: Vec<ThreadMember>,
	},
	Profile {
		user_id: Snowflake,
		raw: Value,
	},
	InviteCreated {
		channel_id: Snowflake,
		raw: Value,
	},
	InvitePreview {
		code: String,
		raw: Value,
	},
	GuildInvites {
		guild_id: Snowflake,
		raw: Value,
	},
	AuditLog {
		guild_id: Snowflake,
		raw: Value,
		append: bool,
	},
	Bans {
		guild_id: Snowflake,
		raw: Value,
	},
	ReactionUsers {
		message_id: Snowflake,
		emoji: String,
		users: Vec<User>,
	},
	GifTrending(GifTrending),
	GifFavorites(GifFavorites),
	GifSearch {
		query: String,
		gifs: Vec<Gif>,
	},
	GifSuggestions {
		query: String,
		terms: Vec<String>,
	},
	StickerPacks(Vec<StickerPack>),
	GuildStickers {
		guild_id: Snowflake,
		stickers: Vec<Sticker>,
	},
	RecentEmoji(Vec<EmojiRef>),
	RecentGifs(Vec<Gif>),
	RecentStickers(Vec<Sticker>),
	UserSettings(Value),
	AuthSessions(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBadge {
	pub id: String,
	pub description: String,
	pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedAccount {
	pub kind: String,
	pub name: String,
	pub verified: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserProfile {
	pub user: User,
	pub bio: Option<String>,
	pub banner: Option<String>,
	pub accent_color: Option<i64>,
	pub pronouns: Option<String>,
	pub badges: Vec<ProfileBadge>,
	pub mutual_guilds: Vec<Snowflake>,
	pub mutual_friends: Vec<Snowflake>,
	pub mutual_friends_count: u32,
	pub connected_accounts: Vec<ConnectedAccount>,
	pub guild_member: Option<Member>,
	pub premium_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteInfo {
	pub code: String,
	pub channel_id: Option<Snowflake>,
	pub inviter_id: Option<Snowflake>,
	pub uses: i64,
	pub max_uses: i64,
	pub max_age: i64,
	pub temporary: bool,
	pub created_at: Option<DateTime<Utc>>,
	pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitePreview {
	pub code: String,
	pub guild_id: Option<Snowflake>,
	pub guild_name: String,
	pub guild_icon: Option<String>,
	pub description: Option<String>,
	pub member_count: i64,
	pub online_count: i64,
	pub channel_id: Option<Snowflake>,
	pub channel_name: Option<String>,
	pub inviter_id: Option<Snowflake>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditChange {
	pub key: String,
	pub old: Option<Value>,
	pub new: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
	pub id: Snowflake,
	pub user_id: Option<Snowflake>,
	pub user: Option<Arc<User>>,
	pub action_type: i64,
	pub action: &'static str,
	pub target_id: Option<Snowflake>,
	pub changes: Vec<AuditChange>,
	pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditLog {
	pub entries: Vec<AuditEntry>,
	pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BanEntry {
	pub user: Arc<User>,
	pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSession {
	pub id_hash: String,
	pub last_used: Option<DateTime<Utc>>,
	pub os: String,
	pub platform: String,
	pub location: String,
}

#[derive(Debug, Default)]
pub(crate) struct ArchivedThreads {
	pub(crate) ids: Vec<Snowflake>,
	pub(crate) has_more: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Lazy {
	pub(crate) pins: HashMap<Snowflake, Vec<Arc<Message>>>,
	pub(crate) archived: HashMap<Snowflake, ArchivedThreads>,
	pub(crate) profiles: HashMap<Snowflake, UserProfile>,
	pub(crate) invites_created: HashMap<Snowflake, InviteInfo>,
	pub(crate) invite_previews: HashMap<String, InvitePreview>,
	pub(crate) guild_invites: HashMap<Snowflake, Vec<InviteInfo>>,
	pub(crate) audit_logs: HashMap<Snowflake, AuditLog>,
	pub(crate) bans: HashMap<Snowflake, Vec<BanEntry>>,
	pub(crate) auth_sessions: Option<Vec<AuthSession>>,
	pub(crate) reaction_users: HashMap<(Snowflake, String), Vec<User>>,
	pub(crate) expressions: Expressions,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawBadge {
	id: String,
	description: String,
	icon: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawMutualGuild {
	id: Snowflake,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawConnection {
	#[serde(rename = "type")]
	kind: String,
	name: String,
	verified: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawMeta {
	bio: Option<String>,
	banner: Option<String>,
	accent_color: Option<i64>,
	pronouns: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawProfile {
	user: Value,
	user_profile: Option<RawMeta>,
	badges: Option<LenientVec<RawBadge>>,
	mutual_guilds: Option<LenientVec<RawMutualGuild>>,
	mutual_friends: Option<LenientVec<User>>,
	mutual_friends_count: Option<u32>,
	connected_accounts: Option<LenientVec<RawConnection>>,
	guild_member: Option<Member>,
	#[serde(deserialize_with = "timestamp")]
	premium_since: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawNamed {
	id: Option<Snowflake>,
	name: Option<String>,
	icon: Option<String>,
	description: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawInvite {
	code: String,
	channel: Option<RawNamed>,
	guild: Option<RawNamed>,
	inviter: Option<User>,
	uses: i64,
	max_uses: i64,
	max_age: i64,
	temporary: bool,
	#[serde(deserialize_with = "timestamp")]
	created_at: Option<DateTime<Utc>>,
	#[serde(deserialize_with = "timestamp")]
	expires_at: Option<DateTime<Utc>>,
	approximate_member_count: i64,
	approximate_presence_count: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawChange {
	key: String,
	old_value: Option<Value>,
	new_value: Option<Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAuditEntry {
	id: Snowflake,
	user_id: Option<Snowflake>,
	target_id: Option<Snowflake>,
	action_type: i64,
	changes: LenientVec<RawChange>,
	reason: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAuditLog {
	audit_log_entries: LenientVec<RawAuditEntry>,
	users: LenientVec<User>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawBan {
	user: User,
	reason: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawClientInfo {
	os: String,
	platform: String,
	location: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAuthSession {
	id_hash: String,
	#[serde(deserialize_with = "timestamp")]
	approx_last_used_time: Option<DateTime<Utc>>,
	client_info: RawClientInfo,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAuthSessions {
	user_sessions: LenientVec<RawAuthSession>,
}

const AUDIT_PAGE: usize = 50;

pub fn audit_action_label(action_type: i64) -> &'static str {
	match action_type {
		1 => "Server updated",
		10 => "Channel created",
		11 => "Channel updated",
		12 => "Channel deleted",
		13 => "Overwrite created",
		14 => "Overwrite updated",
		15 => "Overwrite deleted",
		20 => "Member kicked",
		21 => "Members pruned",
		22 => "Member banned",
		23 => "Member unbanned",
		24 => "Member updated",
		25 => "Member roles updated",
		26 => "Member moved",
		27 => "Member disconnected",
		30 => "Role created",
		31 => "Role updated",
		32 => "Role deleted",
		40 => "Invite created",
		41 => "Invite updated",
		42 => "Invite deleted",
		50 => "Webhook created",
		51 => "Webhook updated",
		52 => "Webhook deleted",
		60 => "Emoji created",
		61 => "Emoji updated",
		62 => "Emoji deleted",
		72 => "Message deleted",
		73 => "Messages deleted",
		74 => "Message pinned",
		75 => "Message unpinned",
		110 => "Thread created",
		111 => "Thread updated",
		112 => "Thread deleted",
		_ => "Unknown action",
	}
}

fn invite_info(raw: RawInvite) -> InviteInfo {
	InviteInfo {
		code: raw.code,
		channel_id: raw.channel.and_then(|channel| channel.id),
		inviter_id: raw.inviter.map(|user| user.id),
		uses: raw.uses,
		max_uses: raw.max_uses,
		max_age: raw.max_age,
		temporary: raw.temporary,
		created_at: raw.created_at,
		expires_at: raw.expires_at,
	}
}

impl State {
	pub(crate) fn apply_loaded(&mut self, loaded: &Loaded) -> Changes {
		let mut changes = Changes::default();
		match loaded {
			Loaded::Pins { channel_id, messages } => {
				let guild_id = self.channel_guild(*channel_id);
				let shared = messages
					.iter()
					.map(|message| {
						self.ingest_message_people(guild_id, message);
						Arc::new(message.clone())
					})
					.collect();
				self.lazy.pins.insert(*channel_id, shared);
				changes.loaded.insert(*channel_id);
			}
			Loaded::ArchivedThreads {
				channel_id,
				threads,
				members,
				has_more,
				append,
			} => {
				let guild_id = self.channel_guild(*channel_id);
				let ids = self.store_threads(guild_id, threads, members, &mut changes);
				let entry = self.lazy.archived.entry(*channel_id).or_default();
				if !*append {
					entry.ids.clear();
				}
				entry.ids.extend(ids);
				entry.has_more = *has_more;
				changes.loaded.insert(*channel_id);
			}
			Loaded::ActiveThreads {
				guild_id,
				threads,
				members,
			} => {
				self.store_threads(Some(*guild_id), threads, members, &mut changes);
				changes.loaded.insert(*guild_id);
			}
			Loaded::Profile { user_id, raw } => {
				if let Some(profile) = self.parse_profile(raw) {
					self.lazy.profiles.insert(*user_id, profile);
					changes.loaded.insert(*user_id);
				}
			}
			Loaded::InviteCreated { channel_id, raw } => {
				if let Ok(invite) = RawInvite::deserialize(raw) {
					self.lazy.invites_created.insert(*channel_id, invite_info(invite));
					changes.loaded.insert(*channel_id);
				}
			}
			Loaded::InvitePreview { code, raw } => {
				if let Ok(invite) = RawInvite::deserialize(raw) {
					let guild = invite.guild.unwrap_or_default();
					let channel = invite.channel.unwrap_or_default();
					let guild_id = guild.id;
					self.lazy.invite_previews.insert(
						code.clone(),
						InvitePreview {
							code: code.clone(),
							guild_id,
							guild_name: guild.name.unwrap_or_default(),
							guild_icon: guild.icon,
							description: guild.description,
							member_count: invite.approximate_member_count,
							online_count: invite.approximate_presence_count,
							channel_id: channel.id,
							channel_name: channel.name,
							inviter_id: invite.inviter.map(|user| user.id),
						},
					);
					changes.loaded.insert(guild_id.unwrap_or_default());
				}
			}
			Loaded::UserSettings(settings) => return self.apply_user_settings(settings),
			Loaded::AuthSessions(raw) => {
				let parsed = RawAuthSessions::deserialize(raw).unwrap_or_default();
				self.lazy.auth_sessions = Some(
					parsed
						.user_sessions
						.into_iter()
						.map(|session| AuthSession {
							id_hash: session.id_hash,
							last_used: session.approx_last_used_time,
							os: session.client_info.os,
							platform: session.client_info.platform,
							location: session.client_info.location,
						})
						.collect(),
				);
				changes.loaded.insert(self.current_user_id().unwrap_or_default());
			}
			Loaded::GuildInvites { guild_id, raw } => {
				let invites = LenientVec::<RawInvite>::deserialize(raw).unwrap_or_default();
				self.lazy
					.guild_invites
					.insert(*guild_id, invites.into_iter().map(invite_info).collect());
				changes.loaded.insert(*guild_id);
			}
			Loaded::AuditLog {
				guild_id,
				raw,
				append,
			} => {
				let parsed = self.parse_audit_log(raw);
				let log = self.lazy.audit_logs.entry(*guild_id).or_default();
				if !*append {
					log.entries.clear();
				}
				log.has_more = parsed.len() >= AUDIT_PAGE;
				log.entries.extend(parsed);
				changes.loaded.insert(*guild_id);
			}
			Loaded::Bans { guild_id, raw } => {
				let bans = LenientVec::<RawBan>::deserialize(raw).unwrap_or_default();
				let entries = bans
					.into_iter()
					.map(|ban| {
						intern_user(&mut self.users, &ban.user);
						BanEntry {
							user: self
								.users
								.get(&ban.user.id)
								.cloned()
								.unwrap_or_else(|| Arc::new(ban.user)),
							reason: ban.reason,
						}
					})
					.collect();
				self.lazy.bans.insert(*guild_id, entries);
				changes.loaded.insert(*guild_id);
			}
			Loaded::ReactionUsers {
				message_id,
				emoji,
				users,
			} => {
				for user in users {
					intern_user(&mut self.users, user);
				}
				self.lazy
					.reaction_users
					.insert((*message_id, emoji.clone()), users.clone());
				changes.loaded.insert(*message_id);
			}
			Loaded::GifTrending(trending) => {
				self.lazy.expressions.gif_trending = Some(Arc::new(trending.clone()));
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::GifFavorites(favorites) => {
				let current = &self.lazy.expressions.gif_favorites;
				if current
					.as_ref()
					.is_none_or(|known| known.version <= favorites.version)
				{
					self.lazy.expressions.gif_favorites = Some(favorites.into());
				}
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::GifSearch { query, gifs } => {
				self.lazy
					.expressions
					.gif_searches
					.put(query, gifs.as_slice().into());
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::GifSuggestions { query, terms } => {
				self.lazy.expressions.gif_suggestions.put(query, terms.clone());
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::StickerPacks(packs) => {
				self.lazy.expressions.sticker_packs = Some(packs.clone());
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::GuildStickers { guild_id, stickers } => {
				self.lazy
					.expressions
					.guild_stickers
					.insert(*guild_id, stickers.clone());
				changes.loaded.insert(*guild_id);
			}
			Loaded::RecentEmoji(emoji) => {
				self.lazy.expressions.recent_emoji.clone_from(emoji);
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::RecentGifs(gifs) => {
				self.lazy.expressions.recent_gifs.clone_from(gifs);
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::RecentStickers(stickers) => {
				self.lazy.expressions.recent_stickers.clone_from(stickers);
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
		}
		changes
	}

	fn parse_profile(&mut self, raw: &Value) -> Option<UserProfile> {
		let parsed = RawProfile::deserialize(raw).ok()?;
		let user = User::deserialize(&parsed.user).ok()?;
		intern_user(&mut self.users, &user);
		let meta = parsed.user_profile.unwrap_or_default();
		let mutual_friends = parsed.mutual_friends.unwrap_or_default();
		for friend in &mutual_friends {
			intern_user(&mut self.users, friend);
		}
		let mutual_friends_count = parsed
			.mutual_friends_count
			.unwrap_or_else(|| u32::try_from(mutual_friends.len()).unwrap_or(u32::MAX));
		Some(UserProfile {
			bio: meta
				.bio
				.or_else(|| parsed.user.get("bio")?.as_str().map(str::to_owned))
				.filter(|bio| !bio.is_empty()),
			banner: meta.banner.or_else(|| user.banner.clone()),
			accent_color: meta.accent_color.or(user.accent_color),
			pronouns: meta.pronouns.filter(|pronouns| !pronouns.is_empty()),
			badges: parsed
				.badges
				.unwrap_or_default()
				.into_iter()
				.map(|badge| ProfileBadge {
					id: badge.id,
					description: badge.description,
					icon: badge.icon,
				})
				.collect(),
			mutual_guilds: parsed
				.mutual_guilds
				.unwrap_or_default()
				.into_iter()
				.map(|guild| guild.id)
				.collect(),
			mutual_friends: mutual_friends.iter().map(|friend| friend.id).collect(),
			mutual_friends_count,
			connected_accounts: parsed
				.connected_accounts
				.unwrap_or_default()
				.into_iter()
				.map(|account| ConnectedAccount {
					kind: account.kind,
					name: account.name,
					verified: account.verified,
				})
				.collect(),
			guild_member: parsed.guild_member,
			premium_since: parsed.premium_since,
			user,
		})
	}

	fn parse_audit_log(&mut self, raw: &Value) -> Vec<AuditEntry> {
		let Ok(log) = RawAuditLog::deserialize(raw) else {
			return Vec::new();
		};
		for user in log.users.iter() {
			intern_user(&mut self.users, user);
		}
		log.audit_log_entries
			.into_iter()
			.map(|entry| AuditEntry {
				id: entry.id,
				user_id: entry.user_id,
				user: entry.user_id.and_then(|id| self.users.get(&id).cloned()),
				action_type: entry.action_type,
				action: audit_action_label(entry.action_type),
				target_id: entry.target_id,
				changes: entry
					.changes
					.into_iter()
					.map(|change| AuditChange {
						key: change.key,
						old: change.old_value,
						new: change.new_value,
					})
					.collect(),
				reason: entry.reason.filter(|reason| !reason.is_empty()),
			})
			.collect()
	}

	pub fn pins(&self, channel_id: Snowflake) -> Option<&[Arc<Message>]> {
		self.lazy.pins.get(&channel_id).map(Vec::as_slice)
	}

	pub fn archived_threads(&self, channel_id: Snowflake) -> Vec<&Channel> {
		let Some(archived) = self.lazy.archived.get(&channel_id) else {
			return Vec::new();
		};
		archived.ids.iter().filter_map(|id| self.channel(*id)).collect()
	}

	pub fn archived_threads_has_more(&self, channel_id: Snowflake) -> bool {
		self.lazy
			.archived
			.get(&channel_id)
			.is_some_and(|archived| archived.has_more)
	}

	pub fn profile(&self, user_id: Snowflake) -> Option<&UserProfile> {
		self.lazy.profiles.get(&user_id)
	}

	pub fn invite_created(&self, channel_id: Snowflake) -> Option<&InviteInfo> {
		self.lazy.invites_created.get(&channel_id)
	}

	pub fn invite_preview(&self, code: &str) -> Option<&InvitePreview> {
		self.lazy.invite_previews.get(code)
	}

	pub fn guild_invites(&self, guild_id: Snowflake) -> &[InviteInfo] {
		self.lazy
			.guild_invites
			.get(&guild_id)
			.map_or(&[][..], Vec::as_slice)
	}

	pub fn audit_log(&self, guild_id: Snowflake) -> Option<&AuditLog> {
		self.lazy.audit_logs.get(&guild_id)
	}

	pub fn auth_sessions(&self) -> Option<&[AuthSession]> {
		self.lazy.auth_sessions.as_deref()
	}

	pub fn bans(&self, guild_id: Snowflake) -> &[BanEntry] {
		self.lazy.bans.get(&guild_id).map_or(&[][..], Vec::as_slice)
	}

	pub fn reaction_users(&self, message_id: Snowflake, emoji: &str) -> Option<&[User]> {
		self.lazy
			.reaction_users
			.get(&(message_id, emoji.to_owned()))
			.map(Vec::as_slice)
	}
}
