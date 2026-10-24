use crate::api::model::{EmojiInfo, MessageFull, StickerInfo};
use crate::api::{
	ChannelSummary, GuildSummary, HostError, MemberSummary, MessageRef, MessageView, OutgoingMessage, User,
	VoiceInfo,
};

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

	fn register_command(&self, _plugin: &str, _name: &str, _description: &str) -> Result<(), HostError> {
		Ok(())
	}

	fn send_as_user(&self, _plugin: &str, _msg: OutgoingMessage) -> Result<(), HostError> {
		Err(HostError::Unavailable)
	}

	fn list_emojis(&self, _plugin: &str, _channel_id: Option<u64>) -> Result<Vec<EmojiInfo>, HostError> {
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
