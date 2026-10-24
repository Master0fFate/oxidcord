use std::collections::HashSet;

use plugin_sdk::model::{EmojiRef, MessageFull, PickVerdict};
use plugin_sdk::patch::Patch;
use plugin_sdk::{Md, OutgoingMessage, Plugin, PluginExt, SendDecision, client, export_plugin, log};

const EMOJI_HOSTS: [&str; 2] = [
	"https://cdn.discordapp.com/emojis/",
	"https://media.discordapp.net/emojis/",
];
const LINK_SIZE: u32 = 48;
const INLINE_SIZE: u32 = 22;
const CREDIT: &str = "Emoji links shown inline";

#[derive(Default)]
struct FakeNitro;

struct EmojiLink {
	id: u64,
	name: String,
}

fn link_for(id: u64, name: &str, animated: bool) -> String {
	let extension = if animated { "gif" } else { "webp" };
	format!("https://cdn.discordapp.com/emojis/{id}.{extension}?size={LINK_SIZE}&name={name}")
}

fn parse_link(url: &str) -> Option<EmojiLink> {
	let rest = EMOJI_HOSTS.iter().find_map(|host| url.strip_prefix(host))?;
	let (file, query) = rest.split_once('?').unwrap_or((rest, ""));
	let (id, extension) = file.split_once('.')?;
	if !matches!(extension, "webp" | "png" | "gif") {
		return None;
	}
	let id = id.parse().ok()?;
	let name = query
		.split('&')
		.find_map(|pair| pair.strip_prefix("name="))
		.filter(|name| {
			!name.is_empty()
				&& name
					.bytes()
					.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
		})
		.unwrap_or("emoji")
		.to_owned();
	Some(EmojiLink { id, name })
}

struct Token<'a> {
	start: usize,
	end: usize,
	name: &'a str,
	id: u64,
	animated: bool,
}

fn next_token(text: &str, from: usize) -> Option<Token<'_>> {
	let mut search = from;
	while let Some(offset) = text[search..].find('<') {
		let start = search + offset;
		let close = text[start..].find('>')?;
		let end = start + close + 1;
		let inner = &text[start + 1..end - 1];
		let (animated, body) = match inner.strip_prefix("a:") {
			Some(body) => (true, body),
			None => (false, inner.strip_prefix(':').unwrap_or("")),
		};
		if let Some((name, id)) = body.split_once(':')
			&& !name.is_empty()
			&& name
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
			&& let Ok(id) = id.parse()
		{
			return Some(Token {
				start,
				end,
				name,
				id,
				animated,
			});
		}
		search = start + 1;
	}
	None
}

fn usable_ids(channel_id: u64) -> HashSet<u64> {
	match client::list_emojis(Some(channel_id)) {
		Ok(emojis) => emojis
			.into_iter()
			.filter(|info| info.usable)
			.filter_map(|info| info.emoji.id)
			.collect(),
		Err(error) => {
			log::warn!("could not list emojis: {error:?}");
			HashSet::new()
		}
	}
}

fn rewrite(content: &str, usable: &HashSet<u64>) -> Option<String> {
	let mut out = String::with_capacity(content.len());
	let mut cursor = 0;
	let mut changed = false;
	while let Some(token) = next_token(content, cursor) {
		out.push_str(&content[cursor..token.start]);
		if usable.contains(&token.id) {
			out.push_str(&content[token.start..token.end]);
		} else {
			out.push_str(&link_for(token.id, token.name, token.animated));
			changed = true;
		}
		cursor = token.end;
	}
	out.push_str(&content[cursor..]);
	changed.then_some(out)
}

fn inline(nodes: Vec<Md>, found: &mut Vec<u64>) -> Vec<Md> {
	nodes
		.into_iter()
		.map(|node| match node {
			Md::Link {
				url,
				text: None,
				suppress_embed,
			} => match parse_link(&url) {
				Some(link) => {
					found.push(link.id);
					Md::InlineImage {
						url,
						alt: format!(":{}:", link.name),
						width: INLINE_SIZE,
						height: INLINE_SIZE,
					}
				}
				None => Md::Link {
					url,
					text: None,
					suppress_embed,
				},
			},
			Md::Bold(children) => Md::Bold(inline(children, found)),
			Md::Italic(children) => Md::Italic(inline(children, found)),
			Md::Underline(children) => Md::Underline(inline(children, found)),
			Md::Strike(children) => Md::Strike(inline(children, found)),
			Md::Quote(children) => Md::Quote(inline(children, found)),
			Md::Subtext(children) => Md::Subtext(inline(children, found)),
			other => other,
		})
		.collect()
}

fn is_emoji_embed(url: Option<&str>, found: &[u64]) -> bool {
	url.and_then(parse_link)
		.is_some_and(|link| found.contains(&link.id))
}

impl Plugin for FakeNitro {
	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		if next_token(&msg.content, 0).is_none() {
			return SendDecision::Pass;
		}
		match rewrite(&msg.content, &usable_ids(msg.channel_id)) {
			Some(content) => SendDecision::Replace(OutgoingMessage { content, ..msg }),
			None => SendDecision::Pass,
		}
	}
}

impl PluginExt for FakeNitro {
	fn on_emoji_pick(&mut self, emoji: &EmojiRef, _channel_id: u64) -> PickVerdict {
		match emoji.id {
			Some(id) => PickVerdict::Insert(format!("{} ", link_for(id, &emoji.name, emoji.animated))),
			None => PickVerdict::Default,
		}
	}

	fn intercept_message(&mut self, msg: &MessageFull, content: Vec<Md>) -> Patch {
		let mut found = Vec::new();
		let content = inline(content, &mut found);
		if found.is_empty() {
			return Patch::keep();
		}
		let mut patch = Patch::keep().content(content).annotate(CREDIT);
		for (index, embed) in msg.embeds.iter().enumerate() {
			let urls = [
				embed.url.as_deref(),
				embed.thumbnail.as_deref(),
				embed.image.as_deref(),
			];
			if urls.into_iter().any(|url| is_emoji_embed(url, &found)) {
				patch = patch.hide_embed(index);
			}
		}
		patch
	}
}

export_plugin!(FakeNitro, ext);
