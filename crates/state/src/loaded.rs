use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use protocol::lenient::{or_default, timestamp, timestamp_or_seconds};
use protocol::{
	ApplicationCommand, ApplicationCommandType, AutoModRule, Channel, ChannelType, CommandApplication,
	CommandChoice, CommandIndex, Connection, Gif, GifFavorites, GifTrending, Guild, GuildBan,
	InteractionContext, JoinRequestStatus, LenientVec, Member, MemberVerification, Message, Permissions,
	PinnedMessage, ScheduledEvent, Snowflake, SoundboardSound, Sticker, StickerPack, ThreadMember, User,
};
use serde::Deserialize;
use serde_json::Value;

use crate::changes::Changes;
use crate::expressions::{EXPRESSIONS_KEY, EmojiRef, Expressions, stamped_stickers};
use crate::guild::{Application, stripped_member};
use crate::state::{State, intern_user};

pub const INBOX_KEY: Snowflake = Snowflake(1);
const VOICE_REGIONS_KEY: Snowflake = Snowflake(2);
pub const COMMAND_CHOICES_KEY: Snowflake = Snowflake(3);
pub const CONNECTIONS_KEY: Snowflake = Snowflake(4);

#[derive(Debug, Clone)]
pub enum Loaded {
	Mentions {
		roles: bool,
		everyone: bool,
		messages: Vec<Message>,
	},
	MentionDismissed(Snowflake),
	CommandIndex {
		scope: Snowflake,
		index: CommandIndex,
	},
	CommandChoices {
		generation: u64,
		choices: Vec<CommandChoice>,
	},
	Pins {
		channel_id: Snowflake,
		items: Vec<PinnedMessage>,
		has_more: bool,
		append: bool,
	},
	PinsAcked {
		channel_id: Snowflake,
		pinned_at: Option<DateTime<Utc>>,
	},
	ArchivedThreads {
		channel_id: Snowflake,
		threads: Vec<Channel>,
		members: Vec<ThreadMember>,
		has_more: bool,
		private: bool,
		append: bool,
	},
	ActiveThreads {
		guild_id: Snowflake,
		threads: Vec<Channel>,
		members: Vec<ThreadMember>,
	},
	ThreadMember(Box<ThreadMember>),
	Profile {
		user_id: Snowflake,
		guild_id: Option<Snowflake>,
		raw: Value,
	},
	Note {
		user_id: Snowflake,
		note: String,
	},
	ApplicationIcon {
		application_id: Snowflake,
		icon: Option<String>,
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
	MemberVerification {
		guild_id: Snowflake,
		form: MemberVerification,
	},
	Application {
		guild_id: Snowflake,
		name: String,
		icon: Option<String>,
	},
	JoinRequestGuilds(LenientVec<Guild>),
	AuditLog {
		guild_id: Snowflake,
		raw: Value,
		append: bool,
	},
	Bans {
		guild_id: Snowflake,
		raw: Value,
	},
	AutomodRule {
		guild_id: Snowflake,
		raw: Value,
	},
	AutomodRuleDeleted {
		guild_id: Snowflake,
		rule_id: Snowflake,
	},
	AutomodRules {
		guild_id: Snowflake,
		raw: Value,
	},
	RoleCounts {
		guild_id: Snowflake,
		raw: Value,
	},
	ReactionUsers {
		message_id: Snowflake,
		emoji: String,
		burst: bool,
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
	DefaultSounds(Vec<SoundboardSound>),
	GuildSounds {
		guild_id: Snowflake,
		sounds: Vec<SoundboardSound>,
	},
	GuildStickers {
		guild_id: Snowflake,
		stickers: Vec<Sticker>,
	},
	ScheduledEvents {
		guild_id: Snowflake,
		events: Vec<ScheduledEvent>,
		interested: Option<Vec<Snowflake>>,
		requested: Instant,
	},
	RecentEmoji(Vec<EmojiRef>),
	RecentGifs(Vec<Gif>),
	RecentStickers(Vec<Sticker>),
	UserSettings(Value),
	SettingsProto(String),
	AuthSessions(Value),
	Connections(Vec<Connection>),
	ConnectionUpdated(Connection),
	ConnectionRemoved {
		kind: String,
		id: String,
	},
	VoiceRegions(Value),
	User(Box<User>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBadge {
	pub id: String,
	pub description: String,
	pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEffect {
	pub id: Snowflake,
	pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedAccount {
	pub id: String,
	pub kind: String,
	pub name: String,
	pub verified: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuildProfile {
	pub guild_id: Snowflake,
	pub bio: Option<String>,
	pub banner: Option<String>,
	pub accent_color: Option<i64>,
	pub theme_colors: Option<[u32; 2]>,
	pub pronouns: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserProfile {
	pub user: User,
	pub bio: Option<String>,
	pub banner: Option<String>,
	pub accent_color: Option<i64>,
	pub theme_colors: Option<[u32; 2]>,
	pub pronouns: Option<String>,
	pub legacy_username: Option<String>,
	pub private: bool,
	pub guild_profile: Option<GuildProfile>,
	pub badges: Vec<ProfileBadge>,
	pub guild_badges: Vec<ProfileBadge>,
	pub profile_effect: Option<ProfileEffect>,
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

#[derive(Debug, Clone, PartialEq)]
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
	pub event: Option<ScheduledEvent>,
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
pub struct VoiceRegion {
	pub id: String,
	pub name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawVoiceRegion {
	id: String,
	name: String,
	deprecated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSession {
	pub id_hash: String,
	pub last_used: Option<DateTime<Utc>>,
	pub os: String,
	pub platform: String,
	pub location: String,
}

#[derive(Debug, Clone)]
pub struct Pin {
	pub pinned_at: Option<DateTime<Utc>>,
	pub message: Arc<Message>,
}

#[derive(Debug, Default)]
pub(crate) struct PinList {
	pub(crate) pins: Vec<Pin>,
	pub(crate) has_more: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ArchivedThreads {
	pub(crate) ids: Vec<Snowflake>,
	pub(crate) private_ids: Vec<Snowflake>,
	pub(crate) has_more: bool,
	pub(crate) private_has_more: bool,
}

pub const USER_COMMANDS: Snowflake = Snowflake(0);

#[derive(Debug, Clone)]
pub struct AppCommand {
	pub application: Option<Arc<CommandApplication>>,
	pub command: Arc<ApplicationCommand>,
}

impl AppCommand {
	pub fn application_name(&self) -> &str {
		self.application
			.as_ref()
			.map_or("", |application| application.name.as_str())
	}

	pub fn application_icon(&self) -> Option<&str> {
		self.application
			.as_ref()
			.and_then(|application| application.icon.as_deref())
	}
}

#[derive(Debug, Default)]
pub(crate) struct CommandScope {
	applications: Vec<Arc<CommandApplication>>,
	commands: Vec<Arc<ApplicationCommand>>,
}

impl From<&CommandIndex> for CommandScope {
	fn from(index: &CommandIndex) -> Self {
		Self {
			applications: index.applications.iter().cloned().map(Arc::new).collect(),
			commands: index.application_commands.iter().cloned().map(Arc::new).collect(),
		}
	}
}

#[derive(Debug)]
pub(crate) struct CommandChoices {
	generation: u64,
	choices: Vec<CommandChoice>,
}

fn profile_badges(raw: Option<LenientVec<RawBadge>>) -> Vec<ProfileBadge> {
	raw.unwrap_or_default()
		.into_iter()
		.map(|badge| ProfileBadge {
			id: badge.id,
			description: badge.description,
			icon: badge.icon,
		})
		.collect()
}

fn context_allows(command: &ApplicationCommand, in_guild: bool, in_bot_dm: bool) -> bool {
	command.contexts.as_ref().is_none_or(|contexts| {
		contexts.iter().any(|context| match context {
			InteractionContext::Guild => in_guild,
			InteractionContext::BotDm => in_bot_dm,
			InteractionContext::PrivateChannel => !in_guild && !in_bot_dm,
			InteractionContext::Unknown(_) => false,
		})
	})
}

fn member_allows(command: &ApplicationCommand, permissions: Permissions) -> bool {
	match command
		.default_member_permissions
		.as_deref()
		.and_then(|bits| bits.parse::<u64>().ok())
	{
		None => true,
		Some(0) => permissions.contains(Permissions::ADMINISTRATOR),
		Some(bits) => permissions.contains(Permissions::from_bits_truncate(bits)),
	}
}
#[derive(Debug, Default)]
pub(crate) struct Lazy {
	pub(crate) mentions: HashMap<(bool, bool), Vec<Arc<Message>>>,
	pub(crate) pins: HashMap<Snowflake, PinList>,
	pub(crate) archived: HashMap<Snowflake, ArchivedThreads>,
	pub(crate) profiles: HashMap<Snowflake, UserProfile>,
	pub(crate) notes: HashMap<Snowflake, String>,
	pub(crate) application_icons: HashMap<Snowflake, Option<String>>,
	pub(crate) invites_created: HashMap<Snowflake, InviteInfo>,
	pub(crate) invite_previews: HashMap<String, InvitePreview>,
	pub(crate) guild_invites: HashMap<Snowflake, Vec<InviteInfo>>,
	pub(crate) member_verifications: HashMap<Snowflake, MemberVerification>,
	pub(crate) audit_logs: HashMap<Snowflake, AuditLog>,
	pub(crate) bans: HashMap<Snowflake, Vec<BanEntry>>,
	pub(crate) automod_rules: HashMap<Snowflake, Vec<AutoModRule>>,
	pub(crate) role_counts: HashMap<Snowflake, HashMap<Snowflake, u64>>,
	pub(crate) auth_sessions: Option<Vec<AuthSession>>,
	pub(crate) connections: Option<Vec<Connection>>,
	pub(crate) voice_regions: Option<Vec<VoiceRegion>>,
	pub(crate) reaction_users: HashMap<(Snowflake, String, bool), Vec<User>>,
	pub(crate) expressions: Expressions,
	pub(crate) command_scopes: HashMap<Snowflake, CommandScope>,
	pub(crate) command_choices: Option<CommandChoices>,
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
	id: String,
	#[serde(rename = "type")]
	kind: String,
	name: String,
	verified: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawMeta {
	guild_id: Option<Snowflake>,
	bio: Option<String>,
	banner: Option<String>,
	accent_color: Option<i64>,
	#[serde(deserialize_with = "or_default")]
	theme_colors: Option<[u32; 2]>,
	pronouns: Option<String>,
	#[serde(deserialize_with = "or_default")]
	profile_effect: Option<RawEffect>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawEffect {
	id: Snowflake,
	#[serde(deserialize_with = "timestamp_or_seconds")]
	expires_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawProfile {
	user: Value,
	user_profile: Option<RawMeta>,
	guild_member_profile: Option<RawMeta>,
	badges: Option<LenientVec<RawBadge>>,
	guild_badges: Option<LenientVec<RawBadge>>,
	mutual_guilds: Option<LenientVec<RawMutualGuild>>,
	mutual_friends: Option<LenientVec<User>>,
	mutual_friends_count: Option<u32>,
	connected_accounts: Option<LenientVec<RawConnection>>,
	guild_member: Option<Member>,
	#[serde(deserialize_with = "timestamp")]
	premium_since: Option<DateTime<Utc>>,
	#[serde(deserialize_with = "or_default")]
	legacy_username: Option<String>,
	#[serde(deserialize_with = "or_default")]
	private: bool,
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
	#[serde(deserialize_with = "or_default")]
	guild_scheduled_event: Option<ScheduledEvent>,
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
		140 => "AutoMod rule created",
		141 => "AutoMod rule updated",
		142 => "AutoMod rule deleted",
		143 => "AutoMod blocked message",
		144 => "AutoMod flagged message",
		145 => "AutoMod timed out member",
		146 => "AutoMod quarantined member",
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
			Loaded::Mentions {
				roles,
				everyone,
				messages,
			} => {
				let shared = messages
					.iter()
					.map(|message| {
						let guild_id = self.channel_guild(message.channel_id);
						self.ingest_message_people(guild_id, message);
						Arc::new(message.clone())
					})
					.collect();
				self.lazy.mentions.insert((*roles, *everyone), shared);
				changes.loaded.insert(INBOX_KEY);
			}
			Loaded::MentionDismissed(message_id) => {
				for mentions in self.lazy.mentions.values_mut() {
					mentions.retain(|message| message.id != *message_id);
				}
				changes.loaded.insert(INBOX_KEY);
			}
			Loaded::CommandIndex { scope, index } => {
				self.lazy.command_scopes.insert(*scope, CommandScope::from(index));
				changes.loaded.insert(*scope);
			}
			Loaded::CommandChoices { generation, choices } => {
				self.lazy.command_choices = Some(CommandChoices {
					generation: *generation,
					choices: choices.clone(),
				});
				changes.loaded.insert(COMMAND_CHOICES_KEY);
			}
			Loaded::Pins {
				channel_id,
				items,
				has_more,
				append,
			} => {
				let guild_id = self.channel_guild(*channel_id);
				let pins: Vec<Pin> = items
					.iter()
					.map(|item| {
						self.ingest_message_people(guild_id, &item.message);
						Pin {
							pinned_at: item.pinned_at,
							message: Arc::new(item.message.clone()),
						}
					})
					.collect();
				let list = self.lazy.pins.entry(*channel_id).or_default();
				if !*append {
					list.pins.clear();
				}
				let known: HashSet<Snowflake> = list.pins.iter().map(|pin| pin.message.id).collect();
				list.pins
					.extend(pins.into_iter().filter(|pin| !known.contains(&pin.message.id)));
				list.has_more = *has_more;
				changes.loaded.insert(*channel_id);
			}
			Loaded::PinsAcked {
				channel_id,
				pinned_at,
			} => {
				self.set_pins_read(*channel_id, *pinned_at);
				changes.read_state = true;
				changes.loaded.insert(*channel_id);
			}
			Loaded::ArchivedThreads {
				channel_id,
				threads,
				members,
				has_more,
				private,
				append,
			} => {
				let guild_id = self.channel_guild(*channel_id);
				let fetched = self.store_threads(guild_id, threads, members, &mut changes);
				let entry = self.lazy.archived.entry(*channel_id).or_default();
				let (ids, more) = if *private {
					(&mut entry.private_ids, &mut entry.private_has_more)
				} else {
					(&mut entry.ids, &mut entry.has_more)
				};
				if !*append {
					ids.clear();
				}
				for id in fetched {
					if !ids.contains(&id) {
						ids.push(id);
					}
				}
				*more = *has_more;
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
			Loaded::ThreadMember(member) => self.set_thread_member(member, &mut changes),
			Loaded::ApplicationIcon { application_id, icon } => {
				self.lazy.application_icons.insert(*application_id, icon.clone());
				changes.loaded.insert(*application_id);
			}
			Loaded::Profile {
				user_id,
				guild_id,
				raw,
			} => {
				if let Some(profile) = self.parse_profile(raw, *guild_id) {
					self.store_profile_member(*user_id, *guild_id, &profile, &mut changes);
					self.lazy.profiles.insert(*user_id, profile);
					changes.loaded.insert(*user_id);
				}
			}
			Loaded::Note { user_id, note } => {
				self.lazy.notes.entry(*user_id).or_insert_with(|| note.clone());
				changes.loaded.insert(*user_id);
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
							event: invite.guild_scheduled_event,
						},
					);
					changes.loaded.insert(guild_id.unwrap_or_default());
				}
			}
			Loaded::UserSettings(settings) => return self.apply_user_settings(settings),
			Loaded::SettingsProto(encoded) => return self.apply_settings_proto(encoded, false),
			Loaded::User(user) => {
				intern_user(&mut self.users, user);
				changes.loaded.insert(user.id);
			}
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
			Loaded::Connections(connections) => {
				self.lazy.connections = Some(
					connections
						.iter()
						.cloned()
						.map(|connection| Connection {
							access_token: None,
							..connection
						})
						.collect(),
				);
				changes.loaded.insert(CONNECTIONS_KEY);
			}
			Loaded::ConnectionUpdated(updated) => {
				if let Some(connection) = self
					.lazy
					.connections
					.iter_mut()
					.flatten()
					.find(|known| known.kind == updated.kind && known.id == updated.id)
				{
					*connection = Connection {
						access_token: None,
						..updated.clone()
					};
					changes.loaded.insert(CONNECTIONS_KEY);
				}
			}
			Loaded::ConnectionRemoved { kind, id } => {
				if let Some(connections) = &mut self.lazy.connections {
					connections.retain(|known| known.kind != *kind || known.id != *id);
					changes.loaded.insert(CONNECTIONS_KEY);
				}
			}
			Loaded::VoiceRegions(raw) => {
				let regions = LenientVec::<RawVoiceRegion>::deserialize(raw).unwrap_or_default();
				self.lazy.voice_regions = Some(
					regions
						.into_iter()
						.filter(|region| !region.deprecated && !region.id.is_empty())
						.map(|region| VoiceRegion {
							id: region.id,
							name: region.name,
						})
						.collect(),
				);
				changes.loaded.insert(VOICE_REGIONS_KEY);
			}
			Loaded::GuildInvites { guild_id, raw } => {
				let invites = LenientVec::<RawInvite>::deserialize(raw).unwrap_or_default();
				self.lazy
					.guild_invites
					.insert(*guild_id, invites.into_iter().map(invite_info).collect());
				changes.loaded.insert(*guild_id);
			}
			Loaded::MemberVerification { guild_id, form } => {
				self.lazy.member_verifications.insert(*guild_id, form.clone());
				changes.loaded.insert(*guild_id);
			}
			Loaded::Application { guild_id, name, icon } => {
				if !self.guilds.contains_key(guild_id) {
					let application = self.applications.entry(*guild_id).or_default();
					application.name.clone_from(name);
					application.icon.clone_from(icon);
					changes.guild_list = true;
				}
			}
			Loaded::JoinRequestGuilds(guilds) => {
				for guild in guilds.iter().filter(|guild| !self.guilds.contains_key(&guild.id)) {
					let application = self.applications.entry(guild.id).or_default();
					application.name = guild.name.clone().unwrap_or_default();
					application.icon.clone_from(&guild.icon);
				}
				changes.guild_list = true;
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
			Loaded::AutomodRules { guild_id, raw } => {
				let rules = LenientVec::<AutoModRule>::deserialize(raw).unwrap_or_default();
				self.lazy.automod_rules.insert(*guild_id, rules.into_vec());
				changes.loaded.insert(*guild_id);
			}
			Loaded::AutomodRule { guild_id, raw } => {
				if let Ok(rule) = AutoModRule::deserialize(raw) {
					let rules = self.lazy.automod_rules.entry(*guild_id).or_default();
					match rules.iter_mut().find(|known| known.id == rule.id) {
						Some(known) => *known = rule,
						None => rules.push(rule),
					}
					changes.loaded.insert(*guild_id);
				}
			}
			Loaded::AutomodRuleDeleted { guild_id, rule_id } => {
				if let Some(rules) = self.lazy.automod_rules.get_mut(guild_id) {
					rules.retain(|known| known.id != *rule_id);
				}
				changes.loaded.insert(*guild_id);
			}
			Loaded::RoleCounts { guild_id, raw } => {
				self.store_role_counts(*guild_id, raw);
				changes.loaded.insert(*guild_id);
			}
			Loaded::ReactionUsers {
				message_id,
				emoji,
				burst,
				users,
			} => {
				for user in users {
					intern_user(&mut self.users, user);
				}
				self.lazy
					.reaction_users
					.insert((*message_id, emoji.clone(), *burst), users.clone());
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
			Loaded::DefaultSounds(sounds) => {
				self.lazy.expressions.default_sounds = Some(sounds.clone());
				changes.loaded.insert(EXPRESSIONS_KEY);
			}
			Loaded::GuildSounds { guild_id, sounds } => return self.guild_sounds_loaded(*guild_id, sounds),
			Loaded::GuildStickers { guild_id, stickers } => {
				self.lazy
					.expressions
					.guild_stickers
					.insert(*guild_id, stamped_stickers(*guild_id, stickers));
				changes.loaded.insert(*guild_id);
			}
			Loaded::ScheduledEvents {
				guild_id,
				events,
				interested,
				requested,
			} => return self.scheduled_events_loaded(*guild_id, events, interested.as_deref(), *requested),
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

	fn parse_profile(&mut self, raw: &Value, guild_id: Option<Snowflake>) -> Option<UserProfile> {
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
		let profile_effect = parsed
			.guild_member_profile
			.as_ref()
			.and_then(|guild_meta| guild_meta.profile_effect.as_ref())
			.or(meta.profile_effect.as_ref())
			.map(|effect| ProfileEffect {
				id: effect.id,
				expires_at: effect.expires_at,
			});
		let guild_profile = match parsed.guild_member_profile {
			Some(meta) => meta.guild_id.or(guild_id).map(|guild_id| GuildProfile {
				guild_id,
				bio: meta.bio.filter(|bio| !bio.is_empty()),
				banner: meta.banner,
				accent_color: meta.accent_color,
				theme_colors: meta.theme_colors,
				pronouns: meta.pronouns.filter(|pronouns| !pronouns.is_empty()),
			}),
			None => guild_id.map(|guild_id| GuildProfile {
				guild_id,
				..GuildProfile::default()
			}),
		};
		Some(UserProfile {
			guild_profile,
			bio: meta
				.bio
				.or_else(|| parsed.user.get("bio")?.as_str().map(str::to_owned))
				.filter(|bio| !bio.is_empty()),
			banner: meta.banner.or_else(|| user.banner.clone()),
			accent_color: meta.accent_color.or(user.accent_color),
			theme_colors: meta.theme_colors,
			pronouns: meta.pronouns.filter(|pronouns| !pronouns.is_empty()),
			legacy_username: parsed.legacy_username.filter(|name| !name.is_empty()),
			private: parsed.private,
			badges: profile_badges(parsed.badges),
			guild_badges: profile_badges(parsed.guild_badges),
			profile_effect,
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
					id: account.id,
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

	pub(crate) fn ban_added(&mut self, ban: &GuildBan) -> Changes {
		let mut changes = Changes::default();
		intern_user(&mut self.users, &ban.user);
		if let Some(entries) = self.lazy.bans.get_mut(&ban.guild_id)
			&& !entries.iter().any(|entry| entry.user.id == ban.user.id)
			&& let Some(user) = self.users.get(&ban.user.id)
		{
			entries.push(BanEntry {
				user: user.clone(),
				reason: None,
			});
			changes.loaded.insert(ban.guild_id);
		}
		changes
	}

	pub(crate) fn ban_removed(&mut self, ban: &GuildBan) -> Changes {
		let mut changes = Changes::default();
		if let Some(entries) = self.lazy.bans.get_mut(&ban.guild_id) {
			let before = entries.len();
			entries.retain(|entry| entry.user.id != ban.user.id);
			if entries.len() != before {
				changes.loaded.insert(ban.guild_id);
			}
		}
		changes
	}

	fn store_role_counts(&mut self, guild_id: Snowflake, raw: &Value) {
		let mut counts: HashMap<Snowflake, u64> =
			self.guilds.get(&guild_id).map_or_else(HashMap::new, |guild| {
				guild.roles.keys().map(|role_id| (*role_id, 0)).collect()
			});
		for (role_id, count) in raw.as_object().into_iter().flatten() {
			if let (Ok(role_id), Some(count)) = (role_id.parse::<u64>(), count.as_u64()) {
				counts.insert(Snowflake(role_id), count);
			}
		}
		self.lazy.role_counts.insert(guild_id, counts);
	}

	fn store_profile_member(
		&mut self,
		user_id: Snowflake,
		guild_id: Option<Snowflake>,
		profile: &UserProfile,
		changes: &mut Changes,
	) {
		let (Some(guild_id), Some(member)) = (guild_id, &profile.guild_member) else {
			return;
		};
		if let Some(guild) = self.guilds.get_mut(&guild_id) {
			guild.members.insert(user_id, stripped_member(member));
			changes.members.insert(guild_id);
		}
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

	pub fn mentions(&self, roles: bool, everyone: bool) -> Option<&[Arc<Message>]> {
		self.lazy.mentions.get(&(roles, everyone)).map(Vec::as_slice)
	}

	pub fn command_choices(&self, generation: u64) -> Option<&[CommandChoice]> {
		self.lazy
			.command_choices
			.as_ref()
			.filter(|answer| answer.generation == generation)
			.map(|answer| answer.choices.as_slice())
	}

	pub fn app_commands(
		&self,
		kind: ApplicationCommandType,
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
	) -> Vec<AppCommand> {
		let permissions = self.my_permissions(channel_id);
		if guild_id.is_some() && !permissions.contains(Permissions::USE_APPLICATION_COMMANDS) {
			return Vec::new();
		}
		let scopes = [guild_id.unwrap_or(channel_id), USER_COMMANDS];
		let dm_recipient = self
			.channel(channel_id)
			.filter(|channel| channel.kind == ChannelType::Dm && channel.recipient_ids.len() == 1)
			.map(|channel| channel.recipient_ids[0]);
		let mut found: Vec<AppCommand> = Vec::new();
		for scope in scopes {
			let Some(index) = self.lazy.command_scopes.get(&scope) else {
				continue;
			};
			for command in &index.commands {
				let usable = command.kind == kind
					&& !found.iter().any(|known| known.command.id == command.id)
					&& guild_id.is_none_or(|_| member_allows(command, permissions));
				if !usable {
					continue;
				}
				let application = index
					.applications
					.iter()
					.find(|application| application.id == command.application_id);
				let bot_id = application
					.and_then(|application| application.bot_id)
					.unwrap_or(command.application_id);
				if !context_allows(command, guild_id.is_some(), dm_recipient == Some(bot_id)) {
					continue;
				}
				found.push(AppCommand {
					application: application.cloned(),
					command: Arc::clone(command),
				});
			}
		}
		found.sort_by(|left, right| {
			(left.application_name(), &left.command.name)
				.cmp(&(right.application_name(), &right.command.name))
		});
		found
	}

	pub fn pins(&self, channel_id: Snowflake) -> Option<&[Pin]> {
		self.lazy.pins.get(&channel_id).map(|list| list.pins.as_slice())
	}

	pub fn pins_has_more(&self, channel_id: Snowflake) -> bool {
		self.lazy.pins.get(&channel_id).is_some_and(|list| list.has_more)
	}

	pub fn archived_threads(&self, channel_id: Snowflake) -> Vec<&Channel> {
		let Some(archived) = self.lazy.archived.get(&channel_id) else {
			return Vec::new();
		};
		archived
			.ids
			.iter()
			.chain(&archived.private_ids)
			.filter_map(|id| self.channel(*id))
			.collect()
	}

	pub fn archived_has_more(&self, channel_id: Snowflake) -> bool {
		self.lazy
			.archived
			.get(&channel_id)
			.is_some_and(|archived| archived.has_more || archived.private_has_more)
	}

	pub fn archived_tail(&self, channel_id: Snowflake, private: bool) -> Option<&Channel> {
		let archived = self.lazy.archived.get(&channel_id)?;
		let (ids, more) = if private {
			(&archived.private_ids, archived.private_has_more)
		} else {
			(&archived.ids, archived.has_more)
		};
		if !more {
			return None;
		}
		ids.iter()
			.rev()
			.filter_map(|id| self.channel(*id))
			.find(|thread| {
				thread
					.thread_metadata
					.as_ref()
					.is_some_and(|metadata| metadata.archived)
			})
	}

	pub fn profile(&self, user_id: Snowflake) -> Option<&UserProfile> {
		self.lazy.profiles.get(&user_id)
	}

	pub fn note(&self, user_id: Snowflake) -> Option<&str> {
		self.lazy
			.notes
			.get(&user_id)
			.map(String::as_str)
			.filter(|note| !note.is_empty())
	}

	pub(crate) fn set_note(&mut self, user_id: Snowflake, note: &str) {
		self.lazy.notes.insert(user_id, note.to_owned());
	}

	pub fn application_icon(&self, application_id: Snowflake) -> Option<Option<&str>> {
		self.lazy
			.application_icons
			.get(&application_id)
			.map(Option::as_deref)
	}

	pub fn shared_guild(&self, user_id: Snowflake, preferred: Option<Snowflake>) -> Option<Snowflake> {
		let has_member = |guild_id: &Snowflake| {
			self.guilds
				.get(guild_id)
				.is_some_and(|guild| guild.members.contains_key(&user_id))
		};
		preferred
			.filter(|guild_id| self.guilds.contains_key(guild_id))
			.or_else(|| {
				self.profile(user_id)?
					.mutual_guilds
					.iter()
					.copied()
					.find(|guild_id| self.guilds.contains_key(guild_id))
			})
			.or_else(|| self.guilds.keys().copied().find(has_member))
	}

	pub fn invite_created(&self, channel_id: Snowflake) -> Option<&InviteInfo> {
		self.lazy.invites_created.get(&channel_id)
	}

	pub fn invite_preview(&self, code: &str) -> Option<&InvitePreview> {
		self.lazy.invite_previews.get(code)
	}

	pub fn member_verification(&self, guild_id: Snowflake) -> Option<&MemberVerification> {
		self.lazy.member_verifications.get(&guild_id)
	}

	pub fn applications(&self) -> impl Iterator<Item = (Snowflake, &Application)> {
		self.applications
			.iter()
			.map(|(id, application)| (*id, application))
	}

	pub fn application(&self, guild_id: Snowflake) -> Option<&Application> {
		self.applications.get(&guild_id)
	}

	pub fn has_unnamed_application(&self) -> bool {
		self.applications
			.values()
			.any(|application| application.name.is_empty())
	}

	pub fn me_pending(&self, guild_id: Snowflake) -> bool {
		let Some(me) = self.current_user_id() else {
			return false;
		};
		self.guild(guild_id)
			.and_then(|guild| guild.members.get(&me))
			.is_some_and(|member| member.pending == Some(true))
	}

	pub fn join_request_status(&self, guild_id: Snowflake) -> Option<&JoinRequestStatus> {
		self.guild(guild_id)?.join_request.as_ref()
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

	pub fn connections(&self) -> Option<&[Connection]> {
		self.lazy.connections.as_deref()
	}

	pub fn auth_sessions(&self) -> Option<&[AuthSession]> {
		self.lazy.auth_sessions.as_deref()
	}

	pub fn voice_regions(&self) -> Option<&[VoiceRegion]> {
		self.lazy.voice_regions.as_deref()
	}

	pub fn current_auth_session(&self) -> Option<usize> {
		let hash = self.session.auth_session_id_hash.as_deref()?;
		self.auth_sessions()?
			.iter()
			.position(|session| session.id_hash == hash)
	}

	pub fn role_member_counts(&self, guild_id: Snowflake) -> Option<&HashMap<Snowflake, u64>> {
		self.lazy.role_counts.get(&guild_id)
	}

	pub fn bans(&self, guild_id: Snowflake) -> &[BanEntry] {
		self.lazy.bans.get(&guild_id).map_or(&[][..], Vec::as_slice)
	}

	pub fn automod_rules(&self, guild_id: Snowflake) -> &[AutoModRule] {
		self.lazy
			.automod_rules
			.get(&guild_id)
			.map_or(&[][..], Vec::as_slice)
	}

	pub fn reaction_users(&self, message_id: Snowflake, emoji: &str, burst: bool) -> Option<&[User]> {
		self.lazy
			.reaction_users
			.get(&(message_id, emoji.to_owned(), burst))
			.map(Vec::as_slice)
	}
}
