use std::path::PathBuf;

use chrono::{DateTime, Utc};
use plugin_host::api::model::{EmojiRef, UiContext, ViewEvent};
use plugin_host::{Permission, ViewContext};
use protocol::{
	ChannelType, Gif, NotificationLevel, PermissionOverwriteKind, Permissions, Snowflake, StatusKind,
};
use rest::UploadFile;
use rest::api::SearchQuery;
use serde_json::Value;
use voice::video::VideoQuality;
use voice::video::capture::ScreenTarget;

#[derive(Debug)]
pub enum Command {
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
	SendMessage {
		channel_id: Snowflake,
		content: String,
		reply_to: Option<Snowflake>,
		files: Vec<UploadFile>,
		by_plugin: Option<String>,
	},
	EditMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
		content: String,
	},
	DeleteMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	AddReaction {
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: String,
	},
	RemoveReaction {
		channel_id: Snowflake,
		message_id: Snowflake,
		emoji: String,
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
	SubscribeGuild {
		guild_id: Snowflake,
	},
	SearchMembers {
		guild_id: Snowflake,
		query: String,
	},
	OpenDm {
		user_id: Snowflake,
	},
	Search {
		guild_id: Option<Snowflake>,
		channel_id: Option<Snowflake>,
		query: SearchQuery,
	},
	ToggleCategory {
		guild_id: Snowflake,
		category_id: Snowflake,
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
	SetVadThreshold {
		auto: bool,
		threshold_db: f32,
	},
	SetDefaultVideoQuality(VideoQuality),
	StartMicTest {
		seconds: u32,
	},
	StopMicTest,
	RequestToSpeak {
		guild_id: Snowflake,
		channel_id: Snowflake,
		request: bool,
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
	},
	PluginMenuItem {
		plugin_id: String,
		menu: String,
		key: String,
		context: UiContext,
	},
	PluginRenderView {
		point: String,
		context: ViewContext,
	},
	PluginEmojiPick {
		emoji: EmojiRef,
		channel_id: Snowflake,
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
	SetVideoQuality(VideoQuality),
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
	},
	JoinThread {
		thread_id: Snowflake,
	},
	LeaveThread {
		thread_id: Snowflake,
	},
	ArchiveThread {
		thread_id: Snowflake,
		archived: bool,
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
	},
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
	CloseDm {
		channel_id: Snowflake,
	},
	EditGuild {
		guild_id: Snowflake,
		name: Option<String>,
		icon_png: Option<Vec<u8>>,
		description: Option<String>,
	},
	CreateChannel {
		guild_id: Snowflake,
		name: String,
		kind: ChannelType,
		parent_id: Option<Snowflake>,
	},
	EditChannel {
		channel_id: Snowflake,
		name: Option<String>,
		topic: Option<String>,
		nsfw: Option<bool>,
		slowmode: Option<u32>,
		position: Option<i64>,
		parent_id: Option<Snowflake>,
	},
	DeleteChannel {
		channel_id: Snowflake,
	},
	CreateRole {
		guild_id: Snowflake,
		name: String,
		color: u32,
		permissions: Permissions,
		hoist: bool,
		mentionable: bool,
	},
	EditRole {
		guild_id: Snowflake,
		role_id: Snowflake,
		name: Option<String>,
		color: Option<u32>,
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
		image_png: Vec<u8>,
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
	LoadInvites {
		guild_id: Snowflake,
	},
	RevokeInvite {
		guild_id: Snowflake,
		code: String,
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
	DeleteMessagesBulk {
		channel_id: Snowflake,
		ids: Vec<Snowflake>,
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
	SetGuildNotifications {
		guild_id: Option<Snowflake>,
		level: NotificationLevel,
		suppress_everyone: bool,
		suppress_roles: bool,
		mobile_push: bool,
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
	UpdateUserSettings(Value),
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
	},
	ForwardMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
		to_channel_id: Snowflake,
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
	SendSticker {
		channel_id: Snowflake,
		sticker_id: Snowflake,
	},
	LoadGifTrending,
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
		file_png_or_apng: Vec<u8>,
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
	Reconnect,
	Logout,
	Shutdown,
}
