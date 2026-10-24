use std::fmt::Write;

use crate::ast::{GuildNavKind, Node, for_each};

const JUMBO_EMOJI_LIMIT: usize = 30;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
	pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mentions {
	pub users: Vec<u64>,
	pub roles: Vec<u64>,
	pub channels: Vec<u64>,
	pub everyone: bool,
	pub here: bool,
}

impl Document {
	pub fn plain_text(&self) -> String {
		let mut out = String::new();
		write_plain(&self.nodes, &mut out);
		out
	}

	pub fn mentions(&self) -> Mentions {
		let mut mentions = Mentions::default();
		for_each(&self.nodes, &mut |node| match node {
			Node::UserMention(id) => push_unique(&mut mentions.users, *id),
			Node::RoleMention(id) => push_unique(&mut mentions.roles, *id),
			Node::ChannelMention(id) => push_unique(&mut mentions.channels, *id),
			Node::Everyone => mentions.everyone = true,
			Node::Here => mentions.here = true,
			_ => {}
		});
		mentions
	}

	pub fn links(&self) -> Vec<&str> {
		let mut links = Vec::new();
		for_each(&self.nodes, &mut |node| {
			if let Node::Link { url, .. } = node {
				links.push(url.as_str());
			}
		});
		links
	}

	pub fn is_only_emoji(&self) -> bool {
		let mut count = 0;
		for node in &self.nodes {
			match node {
				Node::CustomEmoji { .. } => count += 1,
				Node::LineBreak => {}
				Node::Text(text) => match unicode_emoji_count(text) {
					Some(found) => count += found,
					None => return false,
				},
				_ => return false,
			}
			if count > JUMBO_EMOJI_LIMIT {
				return false;
			}
		}
		count > 0
	}
}

fn push_unique(ids: &mut Vec<u64>, id: u64) {
	if !ids.contains(&id) {
		ids.push(id);
	}
}

fn write_plain(nodes: &[Node], out: &mut String) {
	for (index, node) in nodes.iter().enumerate() {
		if index > 0 && (node.is_block() || nodes[index - 1].is_block()) {
			out.push('\n');
		}
		match node {
			Node::Text(text) | Node::Code(text) => out.push_str(text),
			Node::Bold(children)
			| Node::Italic(children)
			| Node::Underline(children)
			| Node::Strike(children)
			| Node::Quote(children)
			| Node::Subtext(children)
			| Node::Styled { children, .. }
			| Node::Heading { children, .. } => write_plain(children, out),
			Node::InlineImage { alt, .. } => out.push_str(alt),
			Node::Spoiler(_) => out.push_str("[spoiler]"),
			Node::CodeBlock { code, .. } => out.push_str(code),
			Node::List {
				ordered,
				start,
				items,
			} => {
				for (item_index, item) in items.iter().enumerate() {
					if item_index > 0 {
						out.push('\n');
					}
					if *ordered {
						let _ = write!(out, "{}. ", u64::from(*start) + item_index as u64);
					} else {
						out.push_str("- ");
					}
					write_plain(item, out);
				}
			}
			Node::UserMention(id) | Node::RoleMention(id) => {
				let _ = write!(out, "@{id}");
			}
			Node::ChannelMention(id) => {
				let _ = write!(out, "#{id}");
			}
			Node::Everyone => out.push_str("@everyone"),
			Node::Here => out.push_str("@here"),
			Node::CustomEmoji { name, .. } => {
				let _ = write!(out, ":{name}:");
			}
			Node::Timestamp { unix, .. } => {
				let _ = write!(out, "{unix}");
			}
			Node::CommandMention { name, .. } => {
				let _ = write!(out, "/{name}");
			}
			Node::GuildNav(kind) => out.push_str(match kind {
				GuildNavKind::Customize => "Customize",
				GuildNavKind::Browse => "Browse Channels",
				GuildNavKind::Guide => "Server Guide",
				GuildNavKind::LinkedRoles => "Linked Roles",
			}),
			Node::Link {
				text: Some(children), ..
			} => write_plain(children, out),
			Node::Link { url, text: None, .. } => out.push_str(url),
			Node::LineBreak => out.push('\n'),
		}
	}
}

fn unicode_emoji_count(text: &str) -> Option<usize> {
	let mut count = 0;
	let mut joined = false;
	let mut regional_open = false;
	for c in text.chars() {
		match c {
			c if c.is_whitespace() => {}
			'\u{200d}' => joined = true,
			'\u{fe0f}' | '\u{20e3}' | '\u{1f3fb}'..='\u{1f3ff}' | '\u{e0020}'..='\u{e007f}' => {}
			'\u{1f1e6}'..='\u{1f1ff}' => {
				if !regional_open && !joined {
					count += 1;
				}
				regional_open = !regional_open;
				joined = false;
			}
			c if is_emoji_base(c) => {
				if !joined {
					count += 1;
				}
				joined = false;
				regional_open = false;
			}
			_ => return None,
		}
	}
	Some(count)
}

fn is_emoji_base(c: char) -> bool {
	matches!(
		c,
		'\u{a9}'
			| '\u{ae}'
			| '\u{203c}'
			| '\u{2049}'
			| '\u{2122}'
			| '\u{2139}'
			| '\u{2194}'..='\u{21aa}'
			| '\u{231a}'..='\u{23ff}'
			| '\u{24c2}'
			| '\u{25aa}'..='\u{25fe}'
			| '\u{2600}'..='\u{27bf}'
			| '\u{2934}'
			| '\u{2935}'
			| '\u{2b05}'..='\u{2b55}'
			| '\u{3030}'
			| '\u{303d}'
			| '\u{3297}'
			| '\u{3299}'
			| '\u{1f000}'..='\u{1faff}'
	)
}
