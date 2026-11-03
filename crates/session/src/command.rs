use std::ops::Range;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use plugin_host::api::model::{
	ChannelClick, ClientOption, ContextDetail, DmSection, EmojiInfo, EmojiRef, LinkVerdict, MessageClick,
	MessageFull, NotificationSettings, UiContext, UserProfile, ViewEvent,
};
use plugin_host::api::{HostError, User as PluginUser};
use plugin_host::{Navigation, Permission, PickedPath, ViewContext};
use protocol::{
	ApplicationCommand, AutoModRule, ChannelPosition, ChannelType, ComponentType,
	DefaultMessageNotifications, DefaultReaction, ForumLayout, ForumSortOrder, Gif, MemberVerification,
	MessageFlags, Nonce, NotificationLevel, PermissionOverwriteKind, Permissions, PollCreate, Role,
	RoleColors, ScheduledEventKind, ScheduledEventStatus, Snowflake, StatusKind, SystemChannelFlags,
	VideoQualityMode,
};
use rest::UploadFile;
use rest::api::SearchQuery;
use serde_json::Value;
use tokio::sync::oneshot;
use voice::video::VideoQuality;
use voice::video::capture::ScreenTarget;

pub const MEMBER_LIST_WINDOW: u32 = 100;

#[derive(Debug, Clone)]
pub struct ComponentUse {
	pub guild_id: Option<Snowflake>,
	pub channel_id: Snowflake,
	pub message_id: Snowflake,
	pub message_flags: MessageFlags,
	pub application_id: Snowflake,
	pub component_type: ComponentType,
	pub custom_id: String,
	pub values: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct VoiceChannelEdit {
	pub bitrate: Option<u32>,
	pub user_limit: Option<u32>,
	pub rtc_region: Option<Option<String>>,
	pub video_quality_mode: Option<VideoQualityMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleColorsEdit {
	pub primary: u32,
	pub secondary: Option<u32>,
	pub tertiary: Option<u32>,
}

impl RoleColorsEdit {
	pub const HOLOGRAPHIC: Self = Self {
		primary: 11_127_295,
		secondary: Some(16_759_788),
		tertiary: Some(16_761_760),
	};

	pub const fn solid(primary: u32) -> Self {
		Self {
			primary,
			secondary: None,
			tertiary: None,
		}
	}
}

impl From<&Role> for RoleColorsEdit {
	fn from(role: &Role) -> Self {
		let colors = role.colors.as_ref();
		let part = |pick: fn(&RoleColors) -> Option<i64>| {
			colors.and_then(pick).and_then(|color| u32::try_from(color).ok())
		};
		Self {
			primary: role.primary_color(),
			secondary: part(|colors| colors.secondary_color),
			tertiary: part(|colors| colors.tertiary_color),
		}
	}
}

impl From<RoleColorsEdit> for RoleColors {
	fn from(colors: RoleColorsEdit) -> Self {
		Self {
			primary_color: Some(i64::from(colors.primary)),
			secondary_color: colors.secondary.map(i64::from),
			tertiary_color: colors.tertiary.map(i64::from),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleIconEdit {
	Remove,
	Image(Vec<u8>),
	Emoji(String),
}

#[derive(Debug, Clone, Copy)]
pub struct ReplyTarget {
	pub message: Snowflake,
	pub mention: bool,
}

pub struct Reply<T>(oneshot::Sender<T>);

impl<T> Reply<T> {
	pub fn channel() -> (Self, oneshot::Receiver<T>) {
		let (sender, receiver) = oneshot::channel();
		(Self(sender), receiver)
	}

	pub fn send(self, value: T) {
		let _ = self.0.send(value);
	}
}

impl<T> std::fmt::Debug for Reply<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Reply")
	}
}

pub type LoadResult<T> = Result<T, HostError>;

#[derive(Debug)]
pub enum PluginLoad {
	Profile {
		user_id: Snowflake,
		reply: Reply<LoadResult<Option<UserProfile>>>,
	},
	Message {
		channel_id: Snowflake,
		message_id: Snowflake,
		reply: Reply<LoadResult<Option<MessageFull>>>,
	},
	User {
		user_id: Snowflake,
		reply: Reply<LoadResult<Option<PluginUser>>>,
	},
	ReactionUsers {
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: String,
		reply: Reply<LoadResult<Vec<PluginUser>>>,
	},
	Media {
		url: String,
		reply: Reply<LoadResult<Vec<u8>>>,
	},
}

#[derive(Debug, Clone, Default)]
pub struct GuildEdit {
	pub name: Option<String>,
	pub description: Option<String>,
	pub icon_png: Option<Vec<u8>>,
	pub banner_png: Option<Vec<u8>>,
	pub splash_png: Option<Vec<u8>>,
	pub system_channel_id: Option<Option<Snowflake>>,
	pub system_channel_flags: Option<SystemChannelFlags>,
	pub afk_channel_id: Option<Option<Snowflake>>,
	pub afk_timeout: Option<u32>,
	pub default_notifications: Option<DefaultMessageNotifications>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForumTagEdit {
	pub id: Option<Snowflake>,
	pub name: String,
	pub moderated: bool,
	pub emoji_id: Option<Snowflake>,
	pub emoji_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ForumEdit {
	pub available_tags: Option<Vec<ForumTagEdit>>,
	pub default_reaction: Option<Option<DefaultReaction>>,
	pub default_sort_order: Option<ForumSortOrder>,
	pub default_layout: Option<ForumLayout>,
	pub default_thread_slowmode: Option<u32>,
	pub require_tag: Option<bool>,
}

#[derive(Debug)]
pub enum Command {
	PluginLoad(PluginLoad),
	SelectChannel {
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
	},
	LoadOlder {
		channel_id: Snowflake,
	},
	LoadNewer {
		channel_id: Snowflake,
	},
	LoadAround {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	LoadLatest {
		channel_id: Snowflake,
	},
	SendMessage {
		channel_id: Snowflake,
		content: String,
		reply_to: Option<ReplyTarget>,
		files: Vec<UploadFile>,
		stickers: Vec<Snowflake>,
		by_plugin: Option<String>,
		silent: bool,
	},
	RetryPending {
		nonce: Nonce,
	},
	DiscardPending {
		nonce: Nonce,
	},
	SendAsUser {
		plugin_id: String,
		channel_id: Snowflake,
		content: String,
		reply_to: Option<ReplyTarget>,
	},
	EditMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
		content: String,
		keep: Option<Vec<Snowflake>>,
	},
	SuppressEmbeds {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	DeleteMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	AddReaction {
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: String,
		burst: bool,
	},
	RemoveReaction {
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: String,
		burst: bool,
	},
	Typing {
		channel_id: Snowflake,
	},
	AckChannel {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	SetDraft {
		channel_id: Snowflake,
		content: String,
	},
	SetPresence {
		status: StatusKind,
		custom_status: Option<String>,
	},
	SetAfk(bool),
	ActivitySettingsChanged,
	SubscribeGuild {
		guild_id: Snowflake,
	},
	MemberListRange {
		guild_id: Snowflake,
		channel_id: Snowflake,
		visible: Range<u32>,
	},
	SearchMembers {
		guild_id: Snowflake,
		query: String,
	},
	RefreshPresence {
		user_id: Snowflake,
		guild_id: Option<Snowflake>,
	},
	LoadApplicationIcon {
		application_id: Snowflake,
	},
	OpenDm {
		user_id: Snowflake,
	},
	Search {
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		query: SearchQuery,
		request: u64,
	},
	ToggleCategory {
		guild_id: Snowflake,
		category_id: Snowflake,
	},
	ToggleBlockedRun {
		channel_id: Snowflake,
		first: Snowflake,
	},
	JoinVoice {
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
	},
	LeaveVoice,
	SetSelfMute(bool),
	SetSelfDeaf(bool),
	SetVoiceDevices {
		input: Option<String>,
		output: Option<String>,
	},
	SetUserVolume {
		user_id: Snowflake,
		volume: f32,
	},
	SetInputMode(voice::InputMode),
	SetPushToTalk(bool),
	SetInputVolume(f32),
	SetOutputVolume(f32),
	SetNoiseSuppression(bool),
	SetEchoCancellation(bool),
	SetAutoGain(bool),
	SetVadThreshold {
		auto: bool,
		threshold_db: f32,
	},
	SetCameraQuality(VideoQuality),
	StartMicTest {
		seconds: u32,
	},
	StopMicTest,
	RequestToSpeak {
		guild_id: Snowflake,
		channel_id: Snowflake,
		request: bool,
	},
	StartStage {
		channel_id: Snowflake,
		topic: String,
	},
	EditStageTopic {
		channel_id: Snowflake,
		topic: String,
	},
	EndStage {
		channel_id: Snowflake,
	},
	SetSpeaker {
		guild_id: Snowflake,
		channel_id: Snowflake,
		user_id: Snowflake,
		speaker: bool,
	},
	AcceptSpeakInvite {
		guild_id: Snowflake,
		channel_id: Snowflake,
	},
	StartCall {
		channel_id: Snowflake,
	},
	DeclineCall {
		channel_id: Snowflake,
	},
	RunPluginCommand {
		name: String,
		args: String,
		channel_id: Snowflake,
	},
	PluginUiAction {
		plugin_id: String,
		id: String,
		payload: String,
	},
	PluginViewEvent {
		plugin_id: String,
		event: ViewEvent,
		detail: Option<ContextDetail>,
	},
	PluginMenuItem {
		plugin_id: String,
		menu: String,
		key: String,
		context: UiContext,
		detail: Option<ContextDetail>,
	},
	PluginRenderView {
		point: String,
		context: ViewContext,
		detail: Option<ContextDetail>,
	},
	PluginEmojiPick {
		emoji: EmojiRef,
		channel_id: Snowflake,
	},
	PluginAutocompleteEmoji {
		query: String,
		generation: u64,
		candidates: Vec<EmojiInfo>,
	},
	PluginMessageClick(MessageClick),
	PluginChannelClick(ChannelClick),
	PluginLinkClick {
		url: String,
		context: UiContext,
		reply: Reply<LinkVerdict>,
	},
	PluginKeybind {
		chord: String,
	},
	PluginBadgeClick {
		plugin_id: String,
		user_id: Snowflake,
		key: String,
	},
	PluginEnable(String),
	PluginDisable(String),
	PluginReload(String),
	PluginInstall {
		path: PathBuf,
	},
	PluginUninstall(String),
	PluginGrant {
		id: String,
		permissions: Vec<Permission>,
		hosts: Vec<String>,
	},
	PluginSetSetting {
		id: String,
		key: String,
		value: String,
	},
	PluginsChanged,
	CatalogRefresh {
		url: String,
	},
	CatalogInstall {
		id: String,
	},
	ThemeCatalogRefresh {
		url: String,
	},
	ThemeInstall {
		id: String,
	},
	StartCamera {
		device_id: Option<String>,
		quality: VideoQuality,
	},
	StopCamera,
	StartScreenShare {
		target: ScreenTarget,
		quality: VideoQuality,
		with_audio: bool,
	},
	StopScreenShare,
	SetStreamQuality(VideoQuality),
	WatchStream {
		stream_key: String,
	},
	StopWatchingStream {
		stream_key: String,
	},
	CreateThread {
		channel_id: Snowflake,
		message_id: Option<Snowflake>,
		name: String,
		auto_archive_minutes: u32,
		private: bool,
		invitable: bool,
	},
	JoinThread {
		thread_id: Snowflake,
	},
	AddThreadMember {
		thread_id: Snowflake,
		user_id: Snowflake,
	},
	LeaveThread {
		thread_id: Snowflake,
	},
	ArchiveThread {
		thread_id: Snowflake,
		archived: bool,
	},
	LockThread {
		thread_id: Snowflake,
		locked: bool,
	},
	EditThread {
		thread_id: Snowflake,
		name: String,
		slowmode: u32,
		auto_archive_minutes: u32,
	},
	DeleteThread {
		thread_id: Snowflake,
	},
	MuteThread {
		thread_id: Snowflake,
		until: Option<DateTime<Utc>>,
	},
	UnmuteThread {
		thread_id: Snowflake,
	},
	SetThreadNotifications {
		thread_id: Snowflake,
		level: NotificationLevel,
	},
	LoadArchivedThreads {
		channel_id: Snowflake,
	},
	LoadActiveThreads {
		guild_id: Snowflake,
	},
	CreateForumPost {
		channel_id: Snowflake,
		name: String,
		content: String,
		tags: Vec<Snowflake>,
		files: Vec<UploadFile>,
	},
	LoadPins {
		channel_id: Snowflake,
		before: Option<DateTime<Utc>>,
	},
	AckPins {
		channel_id: Snowflake,
	},
	LoadMentions,
	PinMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	UnpinMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	AddFriend {
		username: String,
		discriminator: Option<u16>,
	},
	AcceptFriend {
		user_id: Snowflake,
	},
	RemoveRelationship {
		user_id: Snowflake,
	},
	BlockUser {
		user_id: Snowflake,
	},
	IgnoreUser {
		user_id: Snowflake,
	},
	UnignoreUser {
		user_id: Snowflake,
	},
	SetFriendNickname {
		user_id: Snowflake,
		nickname: Option<String>,
	},
	SetNote {
		user_id: Snowflake,
		note: String,
	},
	LoadProfile {
		user_id: Snowflake,
		guild_id: Option<Snowflake>,
	},
	CreateInvite {
		channel_id: Snowflake,
		max_age: u32,
		max_uses: u32,
		temporary: bool,
	},
	ResolveInvite {
		code: String,
	},
	LoadMemberVerification {
		guild_id: Snowflake,
	},
	SubmitMemberVerification {
		guild_id: Snowflake,
		form: MemberVerification,
	},
	AcceptInvite {
		code: String,
	},
	LeaveGuild {
		guild_id: Snowflake,
	},
	CreateGroupDm {
		recipient_ids: Vec<Snowflake>,
	},
	AddGroupRecipient {
		channel_id: Snowflake,
		user_id: Snowflake,
	},
	RemoveGroupRecipient {
		channel_id: Snowflake,
		user_id: Snowflake,
	},
	RenameGroup {
		channel_id: Snowflake,
		name: String,
	},
	SetGroupIcon {
		channel_id: Snowflake,
		icon_png: Vec<u8>,
	},
	TransferGroup {
		channel_id: Snowflake,
		owner_id: Snowflake,
	},
	CloseDm {
		channel_id: Snowflake,
	},
	AcceptMessageRequest {
		channel_id: Snowflake,
	},
	RejectMessageRequests {
		channel_ids: Vec<Snowflake>,
	},
	EditGuild {
		guild_id: Snowflake,
		edit: GuildEdit,
	},
	DeleteGuild {
		guild_id: Snowflake,
	},
	CreateChannel {
		guild_id: Snowflake,
		name: String,
		kind: ChannelType,
		parent_id: Option<Snowflake>,
		private: bool,
	},
	EditChannel {
		channel_id: Snowflake,
		name: Option<String>,
		topic: Option<String>,
		nsfw: Option<bool>,
		slowmode: Option<u32>,
		parent_id: Option<Option<Snowflake>>,
		voice: VoiceChannelEdit,
		forum: ForumEdit,
	},
	ReorderChannels {
		guild_id: Snowflake,
		positions: Vec<ChannelPosition>,
	},
	SetPostTags {
		thread_id: Snowflake,
		tags: Vec<Snowflake>,
	},
	SetVoiceStatus {
		channel_id: Snowflake,
		status: Option<String>,
	},
	LoadVoiceRegions,
	DeleteChannel {
		channel_id: Snowflake,
	},
	CreateRole {
		guild_id: Snowflake,
		name: String,
		colors: RoleColorsEdit,
		permissions: Permissions,
		hoist: bool,
		mentionable: bool,
	},
	EditRole {
		guild_id: Snowflake,
		role_id: Snowflake,
		name: Option<String>,
		colors: Option<RoleColorsEdit>,
		icon: Option<RoleIconEdit>,
		permissions: Option<Permissions>,
		hoist: Option<bool>,
		mentionable: Option<bool>,
	},
	DeleteRole {
		guild_id: Snowflake,
		role_id: Snowflake,
	},
	ReorderRoles {
		guild_id: Snowflake,
		positions: Vec<(Snowflake, i64)>,
	},
	SetChannelOverwrite {
		channel_id: Snowflake,
		target_id: Snowflake,
		kind: PermissionOverwriteKind,
		allow: Permissions,
		deny: Permissions,
	},
	DeleteChannelOverwrite {
		channel_id: Snowflake,
		target_id: Snowflake,
	},
	CreateEmoji {
		guild_id: Snowflake,
		name: String,
		image: Vec<u8>,
	},
	DeleteEmoji {
		guild_id: Snowflake,
		emoji_id: Snowflake,
	},
	LoadAuditLog {
		guild_id: Snowflake,
		before: Option<Snowflake>,
		action_type: Option<i64>,
	},
	LoadBans {
		guild_id: Snowflake,
	},
	LoadAutomodRules {
		guild_id: Snowflake,
	},
	SaveAutomodRule {
		guild_id: Snowflake,
		rule: Box<AutoModRule>,
		original: Option<Box<AutoModRule>>,
	},
	SetAutomodEnabled {
		guild_id: Snowflake,
		rule_id: Snowflake,
		enabled: bool,
	},
	DeleteAutomodRule {
		guild_id: Snowflake,
		rule_id: Snowflake,
	},
	LoadRoleCounts {
		guild_id: Snowflake,
	},
	LoadInvites {
		guild_id: Snowflake,
	},
	RevokeInvite {
		guild_id: Snowflake,
		code: String,
	},
	LoadScheduledEvents {
		guild_id: Snowflake,
	},
	CreateScheduledEvent {
		guild_id: Snowflake,
		name: String,
		description: Option<String>,
		kind: ScheduledEventKind,
		channel_id: Option<Snowflake>,
		location: Option<String>,
		starts_at: DateTime<Utc>,
		ends_at: Option<DateTime<Utc>>,
	},
	SetScheduledEventStatus {
		guild_id: Snowflake,
		event_id: Snowflake,
		status: ScheduledEventStatus,
	},
	SetScheduledEventInterest {
		guild_id: Snowflake,
		event_id: Snowflake,
		interested: bool,
	},
	KickMember {
		guild_id: Snowflake,
		user_id: Snowflake,
		reason: Option<String>,
	},
	BanMember {
		guild_id: Snowflake,
		user_id: Snowflake,
		delete_message_seconds: u32,
		reason: Option<String>,
	},
	UnbanMember {
		guild_id: Snowflake,
		user_id: Snowflake,
	},
	TimeoutMember {
		guild_id: Snowflake,
		user_id: Snowflake,
		until: Option<DateTime<Utc>>,
	},
	SetNickname {
		guild_id: Snowflake,
		user_id: Snowflake,
		nick: Option<String>,
	},
	AddMemberRole {
		guild_id: Snowflake,
		user_id: Snowflake,
		role_id: Snowflake,
	},
	RemoveMemberRole {
		guild_id: Snowflake,
		user_id: Snowflake,
		role_id: Snowflake,
	},
	MoveMember {
		guild_id: Snowflake,
		user_id: Snowflake,
		channel_id: Option<Snowflake>,
	},
	ServerMuteMember {
		guild_id: Snowflake,
		user_id: Snowflake,
		muted: bool,
	},
	ServerDeafenMember {
		guild_id: Snowflake,
		user_id: Snowflake,
		deafened: bool,
	},
	MuteChannel {
		channel_id: Snowflake,
		guild_id: Option<Snowflake>,
		until: Option<DateTime<Utc>>,
	},
	UnmuteChannel {
		channel_id: Snowflake,
		guild_id: Option<Snowflake>,
	},
	MuteGuild {
		guild_id: Snowflake,
		until: Option<DateTime<Utc>>,
	},
	UnmuteGuild {
		guild_id: Snowflake,
	},
	SetChannelNotifications {
		channel_id: Snowflake,
		level: NotificationLevel,
	},
	MarkGuildRead {
		guild_id: Snowflake,
	},
	MarkChannelRead {
		channel_id: Snowflake,
	},
	MarkUnread {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	SetShowCurrentGame(bool),
	SetFriendSources(u32),
	SetGuildLayout(Vec<state::RailEntry>),
	UpdateGuildSettingsRaw {
		guild_id: Option<Snowflake>,
		changes: Value,
	},
	SwitchAccount(Snowflake),
	CreateGuild {
		name: String,
		icon: Option<String>,
	},
	LoadSessions,
	LogoutSessions(Vec<String>),
	SetCustomStatus {
		text: Option<String>,
		emoji: Option<String>,
		emoji_id: Option<Snowflake>,
		expires: Option<DateTime<Utc>>,
	},
	SetStatus(StatusKind),
	EditProfile {
		global_name: Option<String>,
		bio: Option<String>,
		avatar_png: Option<Vec<u8>>,
		banner_png: Option<Vec<u8>>,
		pronouns: Option<String>,
	},
	SetGuildProfile {
		guild_id: Snowflake,
		nick: Option<String>,
		avatar_png: Option<Vec<u8>>,
		bio: Option<String>,
	},
	LoadReactionUsers {
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: String,
		burst: bool,
	},
	ForwardMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
		to_channel_id: Snowflake,
		comment: Option<String>,
		silent: bool,
	},
	VotePoll {
		channel_id: Snowflake,
		message_id: Snowflake,
		answer_ids: Vec<i64>,
	},
	EndPoll {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	SendPoll {
		channel_id: Snowflake,
		poll: Box<PollCreate>,
	},
	UseComponent(ComponentUse),
	RunMessageCommand {
		guild_id: Option<Snowflake>,
		channel_id: Snowflake,
		message_id: Snowflake,
		command: Box<ApplicationCommand>,
	},
	SaveMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
		due_at: Option<DateTime<Utc>>,
	},
	PublishMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	SendSticker {
		channel_id: Snowflake,
		sticker_id: Snowflake,
	},
	LoadGifTrending,
	LoadGifFavorites,
	FavoriteGif {
		gif: Gif,
	},
	UnfavoriteGif {
		url: String,
	},
	SearchGifs {
		query: String,
	},
	SuggestGifs {
		query: String,
	},
	SelectGif {
		id: String,
		query: String,
	},
	SendGif {
		channel_id: Snowflake,
		gif: Gif,
	},
	LoadStickerPacks,
	LoadGuildStickers {
		guild_id: Snowflake,
	},
	CreateSticker {
		guild_id: Snowflake,
		name: String,
		description: String,
		tags: String,
		file: Vec<u8>,
		filename: String,
	},
	EditSticker {
		guild_id: Snowflake,
		sticker_id: Snowflake,
		name: Option<String>,
		description: Option<String>,
		tags: Option<String>,
	},
	DeleteSticker {
		guild_id: Snowflake,
		sticker_id: Snowflake,
	},
	EditEmoji {
		guild_id: Snowflake,
		id: Snowflake,
		name: String,
		roles: Vec<Snowflake>,
	},
	StartReply {
		channel_id: Snowflake,
		message_id: Snowflake,
		mention: bool,
	},
	StartEdit {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	JumpToMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	JumpToTop {
		channel_id: Snowflake,
	},
	OpenSettings {
		page: String,
	},
	Navigate(Navigation),
	PickFiles {
		multiple: bool,
		reply: Reply<LoadResult<Vec<PickedPath>>>,
	},
	AttachFiles {
		channel_id: Snowflake,
		paths: Vec<PathBuf>,
	},
	AttachSticker {
		channel_id: Snowflake,
		sticker_id: Snowflake,
	},
	SetComposerText {
		channel_id: Snowflake,
		content: String,
	},
	SaveAttachment {
		channel_id: Snowflake,
		message_id: Snowflake,
		attachment_id: Snowflake,
	},
	SaveFile {
		url: String,
		name: String,
	},
	SetClientOption {
		plugin_id: String,
		option: ClientOption,
	},
	SetDmSections {
		plugin_id: String,
		sections: Vec<DmSection>,
	},
	SetNotificationSettings {
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		settings: NotificationSettings,
	},
	CloneEmoji {
		guild_id: Snowflake,
		emoji_id: Snowflake,
		animated: bool,
		name: String,
	},
	CloneSticker {
		guild_id: Snowflake,
		sticker_id: Snowflake,
	},
	Reconnect,
	Logout,
	Shutdown,
}
