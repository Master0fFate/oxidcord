use std::sync::Arc;

use markdown::Node;
use protocol::{Message, PartialEmoji, Snowflake};
use url::Url;

use super::md::rich_to_md;
use super::view::{ImagePolicy, clip, clip_line, sanitize_card, sanitize_color, sanitize_view, valid_icon};
use crate::api::MessageView;
use crate::api::model::{
	Annotation, AttachmentInfo, AttachmentOverride, CodeBlockNode, Decoration, EmbedAction, EmbedInfo,
	EmojiRef, Lifecycle, MessageFull, MessagePatch, MessagePatchExt, ReactionInfo, StickerInfo, StyledSpan,
	Visibility,
};
use crate::patches::PluginPatch;

const MAX_ANNOTATIONS: usize = 4;
const MAX_EXTRA_EMBEDS: usize = 3;
const MAX_HIDDEN_ATTACHMENTS: usize = 10;
const MAX_REASON: usize = 200;
const MAX_EXT_TEXT: usize = 200;
const MAX_ALT: usize = 1_000;
const MAX_ATTACHMENT_OVERRIDES: usize = 10;
const MAX_CODE_LINES: usize = 2_000;
const MAX_CODE_SPANS: usize = 400;
const MAX_SPAN_CHILDREN: usize = 256;
const MAX_LANGUAGE: usize = 32;

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

pub struct ExtContext<'a> {
	pub attachments: &'a [AttachmentInfo],
	pub code_blocks: usize,
}

pub fn sanitize_patch(
	plugin_id: &Arc<str>,
	patch: MessagePatch,
	embed_count: usize,
	images: &ImagePolicy,
) -> SanitizedPatch {
	let (content, retain, plugin_patch) = sanitize_base(plugin_id, patch, embed_count, images);
	SanitizedPatch {
		content,
		retain,
		patch: (!plugin_patch.is_empty()).then_some(plugin_patch),
	}
}

pub fn sanitize_patch_ext(
	plugin_id: &Arc<str>,
	patch: MessagePatchExt,
	embed_count: usize,
	images: &ImagePolicy,
	context: &ExtContext<'_>,
) -> SanitizedPatch {
	let MessagePatchExt {
		base,
		attachments,
		layout,
		reply_annotation,
		reveal_spoilers,
		tooltip,
		code_blocks,
	} = patch;
	let (content, retain, mut plugin_patch) = sanitize_base(plugin_id, base, embed_count, images);
	plugin_patch.attachments = attachment_overrides(attachments, context.attachments);
	plugin_patch.layout = layout;
	plugin_patch.reply_annotation = reply_annotation
		.map(|text| clip_line(&text, MAX_EXT_TEXT))
		.filter(|text| !text.trim().is_empty());
	plugin_patch.reveal_spoilers = reveal_spoilers;
	plugin_patch.tooltip = tooltip
		.map(|text| clip_line(&text, MAX_EXT_TEXT))
		.filter(|text| !text.trim().is_empty());
	plugin_patch.code_blocks = code_block_overrides(code_blocks, context.code_blocks);
	SanitizedPatch {
		content,
		retain,
		patch: (!plugin_patch.is_empty()).then_some(plugin_patch),
	}
}

fn same_host(replacement: &str, original: &str) -> bool {
	let (Ok(replacement), Ok(original)) = (Url::parse(replacement), Url::parse(original)) else {
		return false;
	};
	replacement.scheme() == "https"
		&& replacement.username().is_empty()
		&& replacement.password().is_none()
		&& replacement.host_str().is_some()
		&& replacement.host_str() == original.host_str()
}

fn attachment_overrides(
	overrides: Vec<AttachmentOverride>,
	known: &[AttachmentInfo],
) -> Vec<AttachmentOverride> {
	let mut kept: Vec<AttachmentOverride> = Vec::new();
	for over in overrides {
		if kept.len() == MAX_ATTACHMENT_OVERRIDES {
			break;
		}
		let Some(original) = known.iter().find(|attachment| attachment.id == over.id) else {
			continue;
		};
		if kept.iter().any(|existing| existing.id == over.id) {
			continue;
		}
		let kept_over = AttachmentOverride {
			id: over.id,
			blur: over.blur,
			alt: over
				.alt
				.map(|text| clip_line(&text, MAX_ALT))
				.filter(|text| !text.trim().is_empty()),
			tooltip: over
				.tooltip
				.map(|text| clip_line(&text, MAX_EXT_TEXT))
				.filter(|text| !text.trim().is_empty()),
			url: over.url.filter(|url| same_host(url, &original.url)),
			hidden: over.hidden,
		};
		kept.push(kept_over);
	}
	kept
}

fn code_block_overrides(blocks: Vec<(u32, CodeBlockNode)>, count: usize) -> Vec<(u32, CodeBlockNode)> {
	let mut kept: Vec<(u32, CodeBlockNode)> = Vec::new();
	let mut lines_left = MAX_CODE_LINES;
	for (index, block) in blocks {
		if lines_left == 0 {
			break;
		}
		if index as usize >= count || kept.iter().any(|(taken, _)| *taken == index) {
			continue;
		}
		let lines: Vec<Vec<StyledSpan>> = block
			.lines
			.into_iter()
			.take(lines_left)
			.map(|line| {
				line.into_iter()
					.take(MAX_CODE_SPANS)
					.map(|span| StyledSpan {
						children: span.children.into_iter().take(MAX_SPAN_CHILDREN).collect(),
						color: span.color.as_ref().and_then(sanitize_color),
						background: span.background.as_ref().and_then(sanitize_color),
						mono: span.mono,
					})
					.collect()
			})
			.collect();
		lines_left -= lines.len();
		kept.push((
			index,
			CodeBlockNode {
				language: block
					.language
					.map(|language| clip_line(&language, MAX_LANGUAGE))
					.filter(|language| !language.trim().is_empty()),
				lines,
			},
		));
	}
	kept
}

fn sanitize_base(
	plugin_id: &Arc<str>,
	patch: MessagePatch,
	embed_count: usize,
	images: &ImagePolicy,
) -> (Option<Vec<Node>>, bool, PluginPatch) {
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
		.filter(|tree| !tree.roots.is_empty())
		.map(Arc::new);
	let plugin_patch = PluginPatch {
		plugin_id: Arc::clone(plugin_id),
		visibility,
		annotations,
		decoration,
		embeds,
		extra_embeds,
		hidden_attachments,
		accessory,
		attachments: Vec::new(),
		layout: None,
		reply_annotation: None,
		reveal_spoilers: false,
		tooltip: None,
		code_blocks: Vec::new(),
	};
	(content, patch.retain, plugin_patch)
}

pub fn empty_patch_ext() -> MessagePatchExt {
	MessagePatchExt {
		base: empty_patch(),
		attachments: Vec::new(),
		layout: None,
		reply_annotation: None,
		reveal_spoilers: false,
		tooltip: None,
		code_blocks: Vec::new(),
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
