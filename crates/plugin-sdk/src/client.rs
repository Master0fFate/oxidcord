use crate::bindings::oxidcord::plugin::client;
use crate::model::{EmojiInfo, EmojiRef, MessageFull, StickerInfo};
use crate::types::{HostError, OutgoingMessage};

pub fn list_emojis(channel_id: Option<u64>) -> Result<Vec<EmojiInfo>, HostError> {
	client::list_emojis(channel_id)
}

pub fn emoji_url(emoji: &EmojiRef, size: u32) -> Option<String> {
	client::emoji_url(emoji, size)
}

pub fn list_stickers(guild_id: Option<u64>) -> Result<Vec<StickerInfo>, HostError> {
	client::list_stickers(guild_id)
}

pub fn sticker_url(sticker: &StickerInfo, size: u32) -> String {
	client::sticker_url(sticker, size)
}

pub fn composer_insert(text: &str) -> Result<(), HostError> {
	client::composer_insert(text)
}

pub fn get_message(channel_id: u64, message_id: u64) -> Result<Option<MessageFull>, HostError> {
	client::get_message(channel_id, message_id)
}

pub fn channel_messages(channel_id: u64, limit: u32) -> Result<Vec<MessageFull>, HostError> {
	client::channel_messages_full(channel_id, limit)
}

pub fn send_as_user(msg: &OutgoingMessage) -> Result<(), HostError> {
	client::send_as_user(msg)
}
