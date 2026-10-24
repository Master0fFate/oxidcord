use std::sync::Arc;

use markdown::Node;
use protocol::{Message, PartialEmoji, Snowflake};

use super::md::rich_to_md;
use super::view::{ImagePolicy, clip, sanitize_card, sanitize_color, sanitize_view, valid_icon};
use crate::api::MessageView;
use crate::api::model::{
	Annotation, AttachmentInfo, Decoration, EmbedAction, EmbedInfo, EmojiRef, Lifecycle, MessageFull,
	MessagePatch, ReactionInfo, StickerInfo, Visibility,
};
use crate::patches::PluginPatch;

const MAX_ANNOTATIONS: usize = 4;
const MAX_EXTRA_EMBEDS: usize = 3;
const MAX_HIDDEN_ATTACHMENTS: usize = 10;
const MAX_REASON: usize = 200;

pub fn emoji_ref(emoji: &PartialEmoji) -> EmojiRef {
	EmojiRef {
		id: emoji.id.map(Snowflake::get),
		name: emoji.name.clone().unwrap_or_default(),
		animated: emoji.animated,
	}
}

pub fn message_full(message: &Message, view: MessageView, mine: bool, lifecycle: Lifecycle) -> MessageFull {
	MessageFull {
		view,
		attachments: message
			.attachments
			.iter()
			.map(|attachment| AttachmentInfo {
				id: attachment.id.get(),
				filename: attachment.filename.clone(),
				content_type: attachment.content_type.clone(),
				size: attachment
					.size
					.and_then(|size| u64::try_from(size).ok())
					.unwrap_or(0),
				url: attachment.url.clone().unwrap_or_default(),
				width: attachment.width.and_then(|width| u32::try_from(width).ok()),
				height: attachment.height.and_then(|height| u32::try_from(height).ok()),
			})
			.collect(),
		embeds: message
			.embeds
			.iter()
			.map(|embed| EmbedInfo {
				kind: embed
					.kind
					.as_ref()
					.map_or_else(|| "rich".to_owned(), |kind| kind.as_str().to_owned()),
				title: embed.title.clone(),
				description: embed.description.clone(),
				url: embed.url.clone(),
				color: embed.color.and_then(|color| u32::try_from(color).ok()),
				provider: embed.provider.as_ref().and_then(|provider| provider.name.clone()),
				image: embed.image.as_ref().and_then(|media| media.url.clone()),
				thumbnail: embed.thumbnail.as_ref().and_then(|media| media.url.clone()),
			})
			.collect(),
		stickers: message
			.sticker_items
			.iter()
			.map(|sticker| StickerInfo {
				id: sticker.id.get(),
				name: sticker.name.clone(),
				format: sticker
					.format_type
					.map_or(0, |format| u8::try_from(i64::from(format)).unwrap_or(0)),
				guild_id: None,
			})
			.collect(),
		reactions: message
			.reactions
			.iter()
			.map(|reaction| ReactionInfo {
				emoji: emoji_ref(&reaction.emoji),
				count: u32::try_from(reaction.count).unwrap_or(0),
				me: reaction.me,
			})
			.collect(),
		mentions: message.mentions.iter().map(|user| user.id.get()).collect(),
		mine,
		lifecycle,
	}
}

pub struct SanitizedPatch {
	pub content: Option<Vec<Node>>,
	pub retain: bool,
	pub patch: Option<PluginPatch>,
}

fn reason(text: &str) -> String {
	clip(text, MAX_REASON)
		.chars()
		.filter(|character| !character.is_control())
		.collect()
}

pub fn sanitize_patch(
	plugin_id: &Arc<str>,
	patch: MessagePatch,
	embed_count: usize,
	images: &ImagePolicy,
) -> SanitizedPatch {
	let content = patch.content.as_ref().map(|tree| rich_to_md(tree, images));
	let visibility = match patch.visibility {
		Visibility::Shown => Visibility::Shown,
		Visibility::Hidden(text) => Visibility::Hidden(reason(&text)),
		Visibility::Collapsed(text) => Visibility::Collapsed(reason(&text)),
	};
	let annotations: Vec<Annotation> = patch
		.annotations
		.iter()
		.take(MAX_ANNOTATIONS)
		.map(|annotation| Annotation {
			text: reason(&annotation.text),
			icon: annotation.icon.clone().filter(|icon| valid_icon(icon)),
			tone: annotation.tone,
		})
		.filter(|annotation| !annotation.text.trim().is_empty())
		.collect();
	let decoration = patch.decoration.as_ref().map(|decoration| Decoration {
		accent: decoration.accent.as_ref().and_then(sanitize_color),
		background: decoration.background.as_ref().and_then(sanitize_color),
		faded: decoration.faded,
		strike: decoration.strike,
	});
	let embeds: Vec<EmbedAction> = patch
		.embeds
		.iter()
		.take(embed_count)
		.map(|action| match action {
			EmbedAction::Keep => EmbedAction::Keep,
			EmbedAction::Hide => EmbedAction::Hide,
			EmbedAction::Replace(card) => EmbedAction::Replace(sanitize_card(card, images)),
		})
		.collect();
	let extra_embeds = patch
		.extra_embeds
		.iter()
		.take(MAX_EXTRA_EMBEDS)
		.map(|card| sanitize_card(card, images))
		.collect();
	let mut hidden_attachments = patch.hidden_attachments;
	hidden_attachments.truncate(MAX_HIDDEN_ATTACHMENTS);
	let accessory = patch
		.accessory
		.as_ref()
		.map(|tree| sanitize_view(tree, images))
		.filter(|tree| !tree.roots.is_empty());
	let plugin_patch = PluginPatch {
		plugin_id: Arc::clone(plugin_id),
		visibility,
		annotations,
		decoration,
		embeds,
		extra_embeds,
		hidden_attachments,
		accessory,
	};
	SanitizedPatch {
		content,
		retain: patch.retain,
		patch: (!plugin_patch.is_empty()).then_some(plugin_patch),
	}
}

pub fn empty_patch() -> MessagePatch {
	MessagePatch {
		content: None,
		visibility: Visibility::Shown,
		annotations: Vec::new(),
		decoration: None,
		embeds: Vec::new(),
		extra_embeds: Vec::new(),
		hidden_attachments: Vec::new(),
		accessory: None,
		retain: false,
	}
}
