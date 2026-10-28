use plugin_sdk::model::{
	AttachmentInfo, ChannelDetail, EmbedInfo, EmojiRef, Lifecycle, MemberInfo, MessageFull, MessageMeta,
	ReactionInfo, StickerInfo,
};
use plugin_sdk::{MessageRef, User};

use crate::json::Json;

fn user(user: &User) -> Json {
	Json::Object(vec![
		("id", Json::id(user.id)),
		("username", Json::text(&user.username)),
		("display_name", Json::text(&user.display_name)),
		("avatar", Json::option(user.avatar.as_deref(), Json::text)),
		("bot", Json::Bool(user.bot)),
	])
}

fn attachment(attachment: &AttachmentInfo) -> Json {
	Json::Object(vec![
		("id", Json::id(attachment.id)),
		("filename", Json::text(&attachment.filename)),
		(
			"content_type",
			Json::option(attachment.content_type.as_deref(), Json::text),
		),
		("size", Json::num(attachment.size)),
		("url", Json::text(&attachment.url)),
		("width", Json::option(attachment.width, Json::num)),
		("height", Json::option(attachment.height, Json::num)),
	])
}

fn embed(embed: &EmbedInfo) -> Json {
	Json::Object(vec![
		("kind", Json::text(&embed.kind)),
		("title", Json::option(embed.title.as_deref(), Json::text)),
		(
			"description",
			Json::option(embed.description.as_deref(), Json::text),
		),
		("url", Json::option(embed.url.as_deref(), Json::text)),
		("color", Json::option(embed.color, Json::num)),
		("provider", Json::option(embed.provider.as_deref(), Json::text)),
		("image", Json::option(embed.image.as_deref(), Json::text)),
		("thumbnail", Json::option(embed.thumbnail.as_deref(), Json::text)),
	])
}

fn sticker(sticker: &StickerInfo) -> Json {
	Json::Object(vec![
		("id", Json::id(sticker.id)),
		("name", Json::text(&sticker.name)),
		("format", Json::num(sticker.format)),
		("guild_id", Json::option(sticker.guild_id, Json::id)),
	])
}

fn emoji(emoji: &EmojiRef) -> Json {
	Json::Object(vec![
		("id", Json::option(emoji.id, Json::id)),
		("name", Json::text(&emoji.name)),
		("animated", Json::Bool(emoji.animated)),
	])
}

fn reaction(reaction: &ReactionInfo) -> Json {
	Json::Object(vec![
		("emoji", emoji(&reaction.emoji)),
		("count", Json::num(reaction.count)),
		("me", Json::Bool(reaction.me)),
	])
}

fn lifecycle(lifecycle: &Lifecycle) -> Json {
	match lifecycle {
		Lifecycle::Live => Json::Object(vec![("state", Json::text("live"))]),
		Lifecycle::Edited(previous) => Json::Object(vec![
			("state", Json::text("edited")),
			(
				"previous_contents",
				Json::list(previous, |content| Json::text(content)),
			),
		]),
		Lifecycle::Deleted(at_ms) => Json::Object(vec![
			("state", Json::text("deleted")),
			("deleted_ms", Json::num(at_ms)),
		]),
	}
}

fn message_ref(reference: &MessageRef) -> Json {
	Json::Object(vec![
		("id", Json::id(reference.id)),
		("channel_id", Json::id(reference.channel_id)),
		("guild_id", Json::option(reference.guild_id, Json::id)),
	])
}

fn meta(meta: &MessageMeta) -> Json {
	Json::Object(vec![
		("guild_id", Json::option(meta.guild_id, Json::id)),
		("reply_to", Json::option(meta.reply_to.as_ref(), message_ref)),
		("flags", Json::num(meta.flags)),
		("latency_ms", Json::option(meta.latency_ms, Json::num)),
		("author_color", Json::option(meta.author_color, Json::num)),
		("pinned", Json::Bool(meta.pinned)),
		("edited_ms", Json::option(meta.edited_ms, Json::num)),
		("nsfw_channel", Json::Bool(meta.nsfw_channel)),
	])
}

pub fn message(message: &MessageFull, extra: Option<&MessageMeta>) -> Json {
	let view = &message.view;
	Json::Object(vec![
		("id", Json::id(view.id)),
		("channel_id", Json::id(view.channel_id)),
		("guild_id", Json::option(view.guild_id, Json::id)),
		("author", user(&view.author)),
		("content", Json::text(&view.content)),
		("timestamp_ms", Json::num(view.timestamp_ms)),
		("edited", Json::Bool(view.edited)),
		("reply_to", Json::option(view.reply_to, Json::id)),
		("flags", Json::num(view.flags)),
		("mine", Json::Bool(message.mine)),
		("mentions", Json::list(&message.mentions, |id| Json::id(*id))),
		("attachments", Json::list(&message.attachments, attachment)),
		("embeds", Json::list(&message.embeds, embed)),
		("stickers", Json::list(&message.stickers, sticker)),
		("reactions", Json::list(&message.reactions, reaction)),
		("lifecycle", lifecycle(&message.lifecycle)),
		("meta", Json::option(extra, meta)),
	])
}

pub fn member(member: &MemberInfo) -> Json {
	Json::Object(vec![
		("user", user(&member.user)),
		("nick", Json::option(member.nick.as_deref(), Json::text)),
		("roles", Json::list(&member.roles, |id| Json::id(*id))),
		("top_role", Json::option(member.top_role, Json::id)),
		("color", Json::option(member.color, Json::num)),
		("joined_ms", Json::option(member.joined_ms, Json::num)),
		(
			"timeout_until_ms",
			Json::option(member.timeout_until_ms, Json::num),
		),
		(
			"premium_since_ms",
			Json::option(member.premium_since_ms, Json::num),
		),
		("pending", Json::Bool(member.pending)),
	])
}

pub fn channel(channel: &ChannelDetail) -> Json {
	Json::Object(vec![
		("id", Json::id(channel.id)),
		("guild_id", Json::option(channel.guild_id, Json::id)),
		("name", Json::text(&channel.name)),
		("kind", Json::num(channel.kind)),
		("topic", Json::option(channel.topic.as_deref(), Json::text)),
		("nsfw", Json::Bool(channel.nsfw)),
		("parent_id", Json::option(channel.parent_id, Json::id)),
		("position", Json::num(channel.position)),
		("recipients", Json::list(&channel.recipients, |id| Json::id(*id))),
		("owner_id", Json::option(channel.owner_id, Json::id)),
		("last_message_id", Json::option(channel.last_message_id, Json::id)),
		("rate_limit", Json::num(channel.rate_limit)),
		("created_ms", Json::num(channel.created_ms)),
	])
}
