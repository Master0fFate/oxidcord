use crate::bindings::oxidcord::plugin::client;
use crate::model::{ClientOption, DmSection, EmojiRef, FileBlob, Keybind, NotificationSettings, PickedFile};
use crate::types::{HostError, MessageRef, OutgoingMessage};

pub fn set_status(status: &str) -> Result<(), HostError> {
	client::set_status(status)
}

pub fn set_custom_status(
	text: &str,
	emoji: Option<&EmojiRef>,
	expires_ms: Option<i64>,
) -> Result<(), HostError> {
	client::set_custom_status(text, emoji, expires_ms)
}

pub fn mark_read(channel_id: u64) -> Result<(), HostError> {
	client::mark_read(channel_id)
}

pub fn mark_unread(message: MessageRef) -> Result<(), HostError> {
	client::mark_unread(message)
}

pub fn mark_guild_read(guild_id: u64) -> Result<(), HostError> {
	client::mark_guild_read(guild_id)
}

pub fn start_reply(message: MessageRef, mention: bool) -> Result<(), HostError> {
	client::start_reply(message, mention)
}

pub fn start_edit(message: MessageRef) -> Result<(), HostError> {
	client::start_edit(message)
}

pub fn jump_to_message(message: MessageRef) -> Result<(), HostError> {
	client::jump_to_message(message)
}

pub fn jump_to_top(channel_id: u64) -> Result<(), HostError> {
	client::jump_to_top(channel_id)
}

pub fn edit_message(message: MessageRef, content: &str) -> Result<(), HostError> {
	client::edit_message(message, content)
}

pub fn delete_message(message: MessageRef) -> Result<(), HostError> {
	client::delete_message(message)
}

pub fn react(message: MessageRef, emoji: &EmojiRef, burst: bool) -> Result<(), HostError> {
	client::react(message, emoji, burst)
}

pub fn remove_reaction(message: MessageRef, emoji: &EmojiRef) -> Result<(), HostError> {
	client::remove_reaction(message, emoji)
}

pub fn pick_files(multiple: bool) -> Result<Vec<PickedFile>, HostError> {
	client::pick_files(multiple)
}

pub fn composer_attach_files(handles: &[u64]) -> Result<(), HostError> {
	client::composer_attach_files(handles)
}

pub fn send_message_files(msg: &OutgoingMessage, files: &[FileBlob]) -> Result<(), HostError> {
	client::send_message_files(msg, files)
}

pub fn composer_set_draft(text: &str) -> Result<(), HostError> {
	client::composer_set_draft(text)
}

pub fn composer_attach_sticker(sticker_id: u64) -> Result<(), HostError> {
	client::composer_attach_sticker(sticker_id)
}

pub fn save_attachment(message: MessageRef, attachment_id: u64) -> Result<(), HostError> {
	client::save_attachment(message, attachment_id)
}

pub fn join_voice(guild_id: Option<u64>, channel_id: u64) -> Result<(), HostError> {
	client::join_voice(guild_id, channel_id)
}

pub fn leave_voice() -> Result<(), HostError> {
	client::leave_voice()
}

pub fn set_guild_notifications(guild_id: u64, settings: NotificationSettings) -> Result<(), HostError> {
	client::set_guild_notifications(guild_id, settings)
}

pub fn set_channel_notifications(channel_id: u64, settings: NotificationSettings) -> Result<(), HostError> {
	client::set_channel_notifications(channel_id, settings)
}

pub fn set_client_option(value: ClientOption) -> Result<(), HostError> {
	client::set_client_option(value)
}

pub fn set_dm_sections(sections: &[DmSection]) -> Result<(), HostError> {
	client::set_dm_sections(sections)
}

pub fn clone_emoji(guild_id: u64, emoji: &EmojiRef, name: &str) -> Result<(), HostError> {
	client::clone_emoji(guild_id, emoji, name)
}

pub fn clone_sticker(guild_id: u64, sticker_id: u64) -> Result<(), HostError> {
	client::clone_sticker(guild_id, sticker_id)
}

pub fn register_keybind(bind: &Keybind) -> Result<(), HostError> {
	client::register_keybind(bind)
}

pub fn unregister_keybind(id: &str) -> Result<(), HostError> {
	client::unregister_keybind(id)
}

pub fn speak(text: &str, rate: f32, volume: f32) -> Result<(), HostError> {
	client::speak(text, rate, volume)
}

pub fn invalidate_message(message: MessageRef) -> Result<(), HostError> {
	client::invalidate_message(message)
}

pub fn invalidate_user(user_id: u64) -> Result<(), HostError> {
	client::invalidate_user(user_id)
}

pub fn invalidate_channel(channel_id: u64) -> Result<(), HostError> {
	client::invalidate_channel(channel_id)
}

pub fn open_settings(page: &str) -> Result<(), HostError> {
	client::open_settings(page)
}

pub fn navigate_back() -> Result<(), HostError> {
	client::navigate_back()
}

pub fn navigate_forward() -> Result<(), HostError> {
	client::navigate_forward()
}
