use plugin_sdk::model::{
	AttachmentInfo, AttachmentOverride, EmbedAction, EmbedInfo, MessageFull, MessageMeta,
};
use plugin_sdk::patch::{Patch, PatchExt};
use plugin_sdk::view;
use plugin_sdk::{Md, Plugin, PluginExt, export_plugin};

const MAX_WORDS: usize = 20;
const MAX_NAME_CHARS: usize = 300;
const GIF_EMBED_KIND: &str = "gifv";

#[derive(Default)]
struct BetterGifAltText;

fn hex_value(byte: u8) -> Option<u8> {
	char::from(byte).to_digit(16).map(|digit| digit as u8)
}

fn percent_decode(text: &str) -> String {
	let bytes = text.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		let escaped = if bytes[index] == b'%' && index + 2 < bytes.len() {
			hex_value(bytes[index + 1]).zip(hex_value(bytes[index + 2]))
		} else {
			None
		};
		match escaped {
			Some((high, low)) => {
				out.push(high * 16 + low);
				index += 3;
			}
			None => {
				out.push(bytes[index]);
				index += 1;
			}
		}
	}
	String::from_utf8_lossy(&out).into_owned()
}

fn gif_alt(source: &str) -> String {
	let path = source.split(['?', '#']).next().unwrap_or(source);
	let tail = path.rsplit('/').next().unwrap_or(path);
	let readable: String = percent_decode(tail)
		.chars()
		.filter(|character| !character.is_ascii_digit())
		.collect();
	let base = readable.strip_suffix(".gif").unwrap_or(&readable);
	let mut words: Vec<&str> = base
		.split([',', '-', '_', ' '])
		.filter(|word| !word.is_empty())
		.collect();
	if words.len() > 1 && words.last() == Some(&"gif") {
		words.pop();
	}
	words.truncate(MAX_WORDS);
	let mut name = words.join(" ");
	if name.chars().count() > MAX_NAME_CHARS {
		name = name.chars().take(MAX_NAME_CHARS).collect();
		name.push_str("...");
	}
	if name.is_empty() {
		"GIF".to_owned()
	} else {
		format!("GIF - {name}")
	}
}

fn is_gif(attachment: &AttachmentInfo) -> bool {
	attachment.content_type.as_deref() == Some("image/gif")
		|| attachment.filename.to_ascii_lowercase().ends_with(".gif")
}

fn attachment_alt(attachment: &AttachmentInfo) -> AttachmentOverride {
	AttachmentOverride {
		id: attachment.id,
		blur: false,
		alt: Some(gif_alt(&attachment.filename)),
		tooltip: None,
		url: None,
		hidden: false,
	}
}

fn embed_action(embed: &EmbedInfo) -> EmbedAction {
	if embed.kind != GIF_EMBED_KIND {
		return EmbedAction::Keep;
	}
	let Some(source) = embed.url.as_deref().or(embed.image.as_deref()) else {
		return EmbedAction::Keep;
	};
	let mut card = view::card(gif_alt(source));
	card.url = embed.url.clone();
	card.image = embed.image.clone().or_else(|| embed.thumbnail.clone());
	card.footer = embed.provider.clone();
	EmbedAction::Replace(card)
}

impl Plugin for BetterGifAltText {}

impl PluginExt for BetterGifAltText {
	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		_meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		let patch = msg
			.attachments
			.iter()
			.filter(|attachment| is_gif(attachment))
			.fold(PatchExt::keep(), |patch, attachment| {
				patch.attachment(attachment_alt(attachment))
			});
		let actions: Vec<EmbedAction> = msg.embeds.iter().map(embed_action).collect();
		if actions.iter().all(|action| *action == EmbedAction::Keep) {
			return patch;
		}
		patch.base(Patch::keep().embeds(actions))
	}
}

export_plugin!(BetterGifAltText, ext);
