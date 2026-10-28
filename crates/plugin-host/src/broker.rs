use std::path::PathBuf;

use protocol::StatusKind;

use crate::api::model::{
	AuthSession, ChannelDetail, ClientInfo, ClientOption, DmSection, EmojiInfo, EmojiRef, FileBlob, Folder,
	GuildDetail, MemberInfo, MessageFull, MessageMeta, NotificationSettings, PluginSummary, Presence,
	ReadState, Relationship, RoleInfo, StickerInfo, UserProfile, VoiceDetail,
};
use crate::api::{
	ChannelSummary, GuildSummary, HostError, MemberSummary, MessageRef, MessageView, OutgoingMessage, User,
	VoiceInfo,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalidation {
	Message(u64, u64),
	User(u64),
	Channel(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
	Back,
	Forward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedPath {
	pub path: PathBuf,
	pub name: String,
	pub size: u64,
	pub content_type: Option<String>,
}

pub trait HostBroker: Send + Sync {
	fn send_message(&self, plugin: &str, msg: OutgoingMessage) -> Result<u64, HostError>;

	fn add_reaction(&self, plugin: &str, message: MessageRef, emoji: String) -> Result<(), HostError>;

	fn open_channel(&self, plugin: &str, guild_id: Option<u64>, channel_id: u64) -> Result<(), HostError>;

	fn clipboard_write(&self, plugin: &str, text: String) -> Result<(), HostError>;

	fn notify(&self, plugin: &str, title: String, body: String) -> Result<(), HostError>;

	fn open_url(&self, plugin: &str, url: String) -> Result<(), HostError>;

	fn current_user(&self, plugin: &str) -> Result<User, HostError>;

	fn current_channel(&self, plugin: &str) -> Result<Option<(Option<u64>, u64)>, HostError>;

	fn channel_messages(
		&self,
		plugin: &str,
		channel_id: u64,
		limit: u32,
	) -> Result<Vec<MessageView>, HostError>;

	fn display_name(&self, plugin: &str, guild_id: Option<u64>, user_id: u64) -> Result<String, HostError>;

	fn list_guilds(&self, plugin: &str) -> Result<Vec<GuildSummary>, HostError>;

	fn list_channels(&self, plugin: &str, guild_id: Option<u64>) -> Result<Vec<ChannelSummary>, HostError>;

	fn list_members(&self, plugin: &str, guild_id: u64, limit: u32) -> Result<Vec<MemberSummary>, HostError>;

	fn voice_state(&self, plugin: &str) -> Result<Option<VoiceInfo>, HostError>;

	fn surfaces_changed(&self) {}

	fn hooks_changed(&self) {}

	fn register_command(&self, _plugin: &str, _name: &str, _description: &str) -> Result<(), HostError> {
		Ok(())
	}

	fn send_as_user(&self, _plugin: &str, _msg: OutgoingMessage) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn list_emojis(
		&self,
		_plugin: &str,
		_channel_id: Option<u64>,
		_limit: u32,
	) -> Result<Vec<EmojiInfo>, HostError> {
		Err(HostError::Unavailable)
	}

	fn list_stickers(&self, _plugin: &str, _guild_id: Option<u64>) -> Result<Vec<StickerInfo>, HostError> {
		Err(HostError::Unavailable)
	}

	fn message_full(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_message_id: u64,
	) -> Result<Option<MessageFull>, HostError> {
		Err(HostError::Unavailable)
	}

	fn channel_messages_full(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_limit: u32,
	) -> Result<Vec<MessageFull>, HostError> {
		Err(HostError::Unavailable)
	}

	fn relationships(&self, _plugin: &str) -> Result<Vec<Relationship>, HostError> {
		Ok(Vec::new())
	}

	fn presence(&self, _plugin: &str, _user_id: u64) -> Result<Option<Presence>, HostError> {
		Ok(None)
	}

	fn idle_seconds(&self, _plugin: &str) -> Result<u32, HostError> {
		Err(HostError::Unavailable)
	}

	fn read_state(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_private: bool,
	) -> Result<Option<ReadState>, HostError> {
		Ok(None)
	}

	fn unread_channels(
		&self,
		_plugin: &str,
		_guild_id: Option<u64>,
		_private: bool,
	) -> Result<Vec<ReadState>, HostError> {
		Ok(Vec::new())
	}

	fn channel_muted(&self, _plugin: &str, _channel_id: u64, _private: bool) -> Result<bool, HostError> {
		Ok(false)
	}

	fn roles(&self, _plugin: &str, _guild_id: u64) -> Result<Vec<RoleInfo>, HostError> {
		Ok(Vec::new())
	}

	fn member_info(
		&self,
		_plugin: &str,
		_guild_id: u64,
		_user_id: u64,
	) -> Result<Option<MemberInfo>, HostError> {
		Ok(None)
	}

	fn channel_permissions(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_user_id: Option<u64>,
		_private: bool,
	) -> Result<u64, HostError> {
		Err(HostError::Unavailable)
	}

	fn guild_detail(&self, _plugin: &str, _guild_id: u64) -> Result<Option<GuildDetail>, HostError> {
		Ok(None)
	}

	fn channel_detail(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_private: bool,
	) -> Result<Option<ChannelDetail>, HostError> {
		Ok(None)
	}

	fn folders(&self, _plugin: &str) -> Result<Vec<Folder>, HostError> {
		Ok(Vec::new())
	}

	fn cached_profile(&self, _plugin: &str, _user_id: u64) -> Option<UserProfile> {
		None
	}

	fn load_profile(&self, _plugin: &str, _user_id: u64) -> Result<Option<UserProfile>, HostError> {
		Err(HostError::Unavailable)
	}

	fn sessions(&self, _plugin: &str) -> Result<Vec<AuthSession>, HostError> {
		Err(HostError::Unavailable)
	}

	fn client_info(&self, _plugin: &str) -> Result<ClientInfo, HostError> {
		Err(HostError::Unavailable)
	}

	fn voice_detail(&self, _plugin: &str, _private: bool) -> Result<Option<VoiceDetail>, HostError> {
		Ok(None)
	}

	fn cached_message(&self, _plugin: &str, _channel_id: u64, _message_id: u64) -> Option<MessageFull> {
		None
	}

	fn load_message(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_message_id: u64,
		_private: bool,
	) -> Result<Option<MessageFull>, HostError> {
		Err(HostError::Unavailable)
	}

	fn message_meta(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_private: bool,
	) -> Result<Option<MessageMeta>, HostError> {
		Ok(None)
	}

	fn cached_reaction_users(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_emoji: &EmojiRef,
		_limit: u32,
		_private: bool,
	) -> Option<Vec<User>> {
		None
	}

	fn load_reaction_users(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_emoji: &EmojiRef,
		_limit: u32,
		_private: bool,
	) -> Result<Vec<User>, HostError> {
		Err(HostError::Unavailable)
	}

	fn cached_user(&self, _plugin: &str, _user_id: u64) -> Option<User> {
		None
	}

	fn load_user(&self, _plugin: &str, _user_id: u64) -> Result<Option<User>, HostError> {
		Err(HostError::Unavailable)
	}

	fn attachment_bytes(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_attachment_id: u64,
		_private: bool,
	) -> Result<Vec<u8>, HostError> {
		Err(HostError::Unavailable)
	}

	fn avatar_bytes(&self, _plugin: &str, _user_id: u64, _size: u32) -> Result<Vec<u8>, HostError> {
		Err(HostError::Unavailable)
	}

	fn favorite_emojis(&self, _plugin: &str) -> Result<Vec<EmojiRef>, HostError> {
		Ok(Vec::new())
	}

	fn plugins(&self, _plugin: &str) -> Result<Vec<PluginSummary>, HostError> {
		Ok(Vec::new())
	}

	fn composer_draft(&self, _plugin: &str, _private: bool) -> Result<String, HostError> {
		Ok(String::new())
	}

	fn speak(&self, _plugin: &str, _text: &str, _rate: f32, _volume: f32) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn set_status(&self, _plugin: &str, _status: StatusKind) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn set_custom_status(
		&self,
		_plugin: &str,
		_text: String,
		_emoji: Option<EmojiRef>,
		_expires_ms: Option<i64>,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn mark_read(&self, _plugin: &str, _channel_id: u64, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn mark_unread(&self, _plugin: &str, _message: &MessageRef, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn mark_guild_read(&self, _plugin: &str, _guild_id: u64) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn start_reply(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_mention: bool,
		_private: bool,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn start_edit(&self, _plugin: &str, _message: &MessageRef, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn jump_to_message(&self, _plugin: &str, _message: &MessageRef, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn jump_to_top(&self, _plugin: &str, _channel_id: u64, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn edit_message(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_content: String,
		_private: bool,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn delete_message(&self, _plugin: &str, _message: &MessageRef, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn react(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_emoji: &EmojiRef,
		_burst: bool,
		_private: bool,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn remove_reaction(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_emoji: &EmojiRef,
		_private: bool,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn pick_files(&self, _plugin: &str, _multiple: bool) -> Result<Vec<PickedPath>, HostError> {
		Err(HostError::Unavailable)
	}

	fn attach_files(&self, _plugin: &str, _paths: Vec<PathBuf>, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn send_message_files(
		&self,
		_plugin: &str,
		_msg: OutgoingMessage,
		_files: Vec<FileBlob>,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn set_draft(&self, _plugin: &str, _text: String, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn attach_sticker(&self, _plugin: &str, _sticker_id: u64, _private: bool) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn save_attachment(
		&self,
		_plugin: &str,
		_message: &MessageRef,
		_attachment_id: u64,
		_private: bool,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn join_voice(&self, _plugin: &str, _guild_id: Option<u64>, _channel_id: u64) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn leave_voice(&self, _plugin: &str) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn guild_notifications(
		&self,
		_plugin: &str,
		_guild_id: u64,
		_settings: NotificationSettings,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn channel_notifications(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_settings: NotificationSettings,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn set_client_option(&self, _plugin: &str, _option: ClientOption) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn set_dm_sections(&self, _plugin: &str, _sections: Vec<DmSection>) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn clone_emoji(
		&self,
		_plugin: &str,
		_guild_id: u64,
		_emoji: &EmojiRef,
		_name: String,
	) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn clone_sticker(&self, _plugin: &str, _guild_id: u64, _sticker_id: u64) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn invalidate(&self, _plugin: &str, _target: Invalidation) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn open_settings(&self, _plugin: &str, _page: &str) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn navigate(&self, _plugin: &str, _direction: Navigation) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}
}

pub struct NoopBroker;

impl HostBroker for NoopBroker {
	fn send_message(&self, _plugin: &str, _msg: OutgoingMessage) -> Result<u64, HostError> {
		Err(HostError::Unavailable)
	}

	fn add_reaction(&self, _plugin: &str, _message: MessageRef, _emoji: String) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn open_channel(&self, _plugin: &str, _guild_id: Option<u64>, _channel_id: u64) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn clipboard_write(&self, _plugin: &str, _text: String) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn notify(&self, _plugin: &str, _title: String, _body: String) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn open_url(&self, _plugin: &str, _url: String) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn current_user(&self, _plugin: &str) -> Result<User, HostError> {
		Err(HostError::Unavailable)
	}

	fn current_channel(&self, _plugin: &str) -> Result<Option<(Option<u64>, u64)>, HostError> {
		Err(HostError::Unavailable)
	}

	fn channel_messages(
		&self,
		_plugin: &str,
		_channel_id: u64,
		_limit: u32,
	) -> Result<Vec<MessageView>, HostError> {
		Err(HostError::Unavailable)
	}

	fn display_name(
		&self,
		_plugin: &str,
		_guild_id: Option<u64>,
		_user_id: u64,
	) -> Result<String, HostError> {
		Err(HostError::Unavailable)
	}

	fn list_guilds(&self, _plugin: &str) -> Result<Vec<GuildSummary>, HostError> {
		Err(HostError::Unavailable)
	}

	fn list_channels(&self, _plugin: &str, _guild_id: Option<u64>) -> Result<Vec<ChannelSummary>, HostError> {
		Err(HostError::Unavailable)
	}

	fn list_members(
		&self,
		_plugin: &str,
		_guild_id: u64,
		_limit: u32,
	) -> Result<Vec<MemberSummary>, HostError> {
		Err(HostError::Unavailable)
	}

	fn voice_state(&self, _plugin: &str) -> Result<Option<VoiceInfo>, HostError> {
		Err(HostError::Unavailable)
	}
}
