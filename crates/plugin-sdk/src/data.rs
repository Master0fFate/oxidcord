use crate::bindings::oxidcord::plugin::client;
use crate::model::{
	AuthSession, ChannelDetail, ClientInfo, EmojiRef, Folder, GuildDetail, MemberInfo, MessageFull,
	MessageMeta, PluginSummary, Presence, ReadState, Relationship, RoleInfo, UserProfile, VoiceDetail,
};
use crate::types::{HostError, MessageRef, User};

pub fn list_relationships() -> Result<Vec<Relationship>, HostError> {
	client::list_relationships()
}

pub fn user_presence(user_id: u64) -> Result<Option<Presence>, HostError> {
	client::user_presence(user_id)
}

pub fn idle_seconds() -> Result<u32, HostError> {
	client::idle_seconds()
}

pub fn read_state(channel_id: u64) -> Result<Option<ReadState>, HostError> {
	client::read_state(channel_id)
}

pub fn unread_channels(guild_id: Option<u64>) -> Result<Vec<ReadState>, HostError> {
	client::unread_channels(guild_id)
}

pub fn channel_muted(channel_id: u64) -> Result<bool, HostError> {
	client::channel_muted(channel_id)
}

pub fn list_roles(guild_id: u64) -> Result<Vec<RoleInfo>, HostError> {
	client::list_roles(guild_id)
}

pub fn member_info(guild_id: u64, user_id: u64) -> Result<Option<MemberInfo>, HostError> {
	client::member_info(guild_id, user_id)
}

pub fn channel_permissions(channel_id: u64, user_id: Option<u64>) -> Result<u64, HostError> {
	client::channel_permissions(channel_id, user_id)
}

pub fn guild_detail(guild_id: u64) -> Result<Option<GuildDetail>, HostError> {
	client::guild_detail(guild_id)
}

pub fn channel_detail(channel_id: u64) -> Result<Option<ChannelDetail>, HostError> {
	client::channel_detail(channel_id)
}

pub fn list_folders() -> Result<Vec<Folder>, HostError> {
	client::list_folders()
}

pub fn user_profile(user_id: u64) -> Result<Option<UserProfile>, HostError> {
	client::user_profile(user_id)
}

pub fn list_sessions() -> Result<Vec<AuthSession>, HostError> {
	client::list_sessions()
}

pub fn client_info() -> Result<ClientInfo, HostError> {
	client::client_info()
}

pub fn voice_detail() -> Result<Option<VoiceDetail>, HostError> {
	client::voice_detail()
}

pub fn fetch_message(channel_id: u64, message_id: u64) -> Result<Option<MessageFull>, HostError> {
	client::fetch_message(channel_id, message_id)
}

pub fn message_meta(message: MessageRef) -> Result<Option<MessageMeta>, HostError> {
	client::message_meta(message)
}

pub fn reaction_users(message: MessageRef, emoji: &EmojiRef, limit: u32) -> Result<Vec<User>, HostError> {
	client::reaction_users(message, emoji, limit)
}

pub fn fetch_user(user_id: u64) -> Result<Option<User>, HostError> {
	client::fetch_user(user_id)
}

pub fn attachment_bytes(message: MessageRef, attachment_id: u64) -> Result<Vec<u8>, HostError> {
	client::attachment_bytes(message, attachment_id)
}

pub fn avatar_bytes(user_id: u64, size: u32) -> Result<Vec<u8>, HostError> {
	client::avatar_bytes(user_id, size)
}

pub fn list_favorite_emojis() -> Result<Vec<EmojiRef>, HostError> {
	client::list_favorite_emojis()
}

pub fn list_plugins() -> Result<Vec<PluginSummary>, HostError> {
	client::list_plugins()
}

pub fn composer_get_draft() -> Result<String, HostError> {
	client::composer_get_draft()
}
