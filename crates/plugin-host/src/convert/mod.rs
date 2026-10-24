use markdown::{GuildNavKind, Node, TimestampStyle};

use crate::api::{
	MdCodeBlock, MdCommand, MdEmoji, MdHeading, MdLink, MdList, MdNode, MdTimestamp, MdTree, MessageView,
	UiNode, UiTree, User,
};

const MAX_MD_NODES: usize = 50_000;
const MAX_MD_DEPTH: usize = 64;
const MAX_UI_NODES: usize = 2_000;
const MAX_UI_STRING: usize = 4_096;

pub fn md_to_tree(nodes: &[Node]) -> MdTree {
	let mut flat = Vec::new();
	let roots = push_all(&mut flat, nodes);
	MdTree { nodes: flat, roots }
}

fn push_all(out: &mut Vec<MdNode>, nodes: &[Node]) -> Vec<u32> {
	nodes.iter().map(|node| push(out, node)).collect()
}

fn push(out: &mut Vec<MdNode>, node: &Node) -> u32 {
	let index = out.len();
	out.push(MdNode::LineBreak);
	let flat = match node {
		Node::Text(text) => MdNode::Text(text.clone()),
		Node::Bold(children) => MdNode::Bold(push_all(out, children)),
		Node::Italic(children) => MdNode::Italic(push_all(out, children)),
		Node::Underline(children) => MdNode::Underline(push_all(out, children)),
		Node::Strike(children) => MdNode::Strike(push_all(out, children)),
		Node::Spoiler(children) => MdNode::Spoiler(push_all(out, children)),
		Node::Code(code) => MdNode::Code(code.clone()),
		Node::CodeBlock { lang, code } => MdNode::CodeBlock(MdCodeBlock {
			lang: lang.clone(),
			code: code.clone(),
		}),
		Node::Quote(children) => MdNode::Quote(push_all(out, children)),
		Node::Heading { level, children } => MdNode::Heading(MdHeading {
			level: *level,
			children: push_all(out, children),
		}),
		Node::Subtext(children) => MdNode::Subtext(push_all(out, children)),
		Node::List {
			ordered,
			start,
			items,
		} => MdNode::List(MdList {
			ordered: *ordered,
			start: *start,
			items: items.iter().map(|item| push_all(out, item)).collect(),
		}),
		Node::UserMention(id) => MdNode::UserMention(*id),
		Node::RoleMention(id) => MdNode::RoleMention(*id),
		Node::ChannelMention(id) => MdNode::ChannelMention(*id),
		Node::Everyone => MdNode::Everyone,
		Node::Here => MdNode::Here,
		Node::CustomEmoji { name, id, animated } => MdNode::CustomEmoji(MdEmoji {
			name: name.clone(),
			id: *id,
			animated: *animated,
		}),
		Node::Timestamp { unix, style } => MdNode::Timestamp(MdTimestamp {
			unix: *unix,
			style: style_code(*style).to_owned(),
		}),
		Node::CommandMention { name, id } => MdNode::CommandMention(MdCommand {
			name: name.clone(),
			id: *id,
		}),
		Node::GuildNav(kind) => MdNode::GuildNav(nav_name(*kind).to_owned()),
		Node::Link {
			url,
			text,
			suppress_embed,
		} => MdNode::Link(MdLink {
			url: url.clone(),
			text: text.as_deref().map(|children| push_all(out, children)),
			suppress_embed: *suppress_embed,
		}),
		Node::LineBreak => MdNode::LineBreak,
	};
	out[index] = flat;
	index as u32
}

fn style_code(style: TimestampStyle) -> &'static str {
	match style {
		TimestampStyle::ShortTime => "t",
		TimestampStyle::LongTime => "T",
		TimestampStyle::ShortDate => "d",
		TimestampStyle::LongDate => "D",
		TimestampStyle::ShortDateTime => "f",
		TimestampStyle::LongDateTime => "F",
		TimestampStyle::Relative => "R",
	}
}

fn style_from_code(code: &str) -> TimestampStyle {
	match code {
		"t" => TimestampStyle::ShortTime,
		"T" => TimestampStyle::LongTime,
		"d" => TimestampStyle::ShortDate,
		"D" => TimestampStyle::LongDate,
		"F" => TimestampStyle::LongDateTime,
		"R" => TimestampStyle::Relative,
		_ => TimestampStyle::ShortDateTime,
	}
}

fn nav_name(kind: GuildNavKind) -> &'static str {
	match kind {
		GuildNavKind::Customize => "customize",
		GuildNavKind::Browse => "browse",
		GuildNavKind::Guide => "guide",
		GuildNavKind::LinkedRoles => "linked-roles",
	}
}

fn nav_from_name(name: &str) -> Option<GuildNavKind> {
	match name {
		"customize" => Some(GuildNavKind::Customize),
		"browse" => Some(GuildNavKind::Browse),
		"guide" => Some(GuildNavKind::Guide),
		"linked-roles" => Some(GuildNavKind::LinkedRoles),
		_ => None,
	}
}

pub fn tree_to_md(tree: &MdTree) -> Option<Vec<Node>> {
	let mut reader = MdReader {
		nodes: &tree.nodes,
		budget: MAX_MD_NODES,
	};
	reader.read_all(&tree.roots, 0, 0)
}

struct MdReader<'a> {
	nodes: &'a [MdNode],
	budget: usize,
}

impl MdReader<'_> {
	fn read_all(&mut self, indices: &[u32], min_index: usize, depth: usize) -> Option<Vec<Node>> {
		if depth > MAX_MD_DEPTH {
			return None;
		}
		let mut out = Vec::with_capacity(indices.len());
		for &index in indices {
			let index = index as usize;
			if index < min_index || index >= self.nodes.len() || self.budget == 0 {
				return None;
			}
			self.budget -= 1;
			if let Some(node) = self.read(index, depth)? {
				out.push(node);
			}
		}
		Some(out)
	}

	fn read(&mut self, index: usize, depth: usize) -> Option<Option<Node>> {
		let child_min = index + 1;
		let child_depth = depth + 1;
		let node = match &self.nodes[index] {
			MdNode::Text(text) => Node::Text(text.clone()),
			MdNode::Bold(children) => Node::Bold(self.read_all(children, child_min, child_depth)?),
			MdNode::Italic(children) => Node::Italic(self.read_all(children, child_min, child_depth)?),
			MdNode::Underline(children) => {
				Node::Underline(self.read_all(children, child_min, child_depth)?)
			}
			MdNode::Strike(children) => Node::Strike(self.read_all(children, child_min, child_depth)?),
			MdNode::Spoiler(children) => Node::Spoiler(self.read_all(children, child_min, child_depth)?),
			MdNode::Code(code) => Node::Code(code.clone()),
			MdNode::CodeBlock(block) => Node::CodeBlock {
				lang: block.lang.clone(),
				code: block.code.clone(),
			},
			MdNode::Quote(children) => Node::Quote(self.read_all(children, child_min, child_depth)?),
			MdNode::Heading(heading) => Node::Heading {
				level: heading.level.clamp(1, 3),
				children: self.read_all(&heading.children, child_min, child_depth)?,
			},
			MdNode::Subtext(children) => Node::Subtext(self.read_all(children, child_min, child_depth)?),
			MdNode::List(list) => {
				let mut items = Vec::with_capacity(list.items.len());
				for item in &list.items {
					items.push(self.read_all(item, child_min, child_depth)?);
				}
				Node::List {
					ordered: list.ordered,
					start: list.start,
					items,
				}
			}
			MdNode::UserMention(id) => Node::UserMention(*id),
			MdNode::RoleMention(id) => Node::RoleMention(*id),
			MdNode::ChannelMention(id) => Node::ChannelMention(*id),
			MdNode::Everyone => Node::Everyone,
			MdNode::Here => Node::Here,
			MdNode::CustomEmoji(emoji) => Node::CustomEmoji {
				name: emoji.name.clone(),
				id: emoji.id,
				animated: emoji.animated,
			},
			MdNode::Timestamp(timestamp) => Node::Timestamp {
				unix: timestamp.unix,
				style: style_from_code(&timestamp.style),
			},
			MdNode::CommandMention(command) => Node::CommandMention {
				name: command.name.clone(),
				id: command.id,
			},
			MdNode::GuildNav(name) => match nav_from_name(name) {
				Some(kind) => Node::GuildNav(kind),
				None => return Some(None),
			},
			MdNode::Link(link) => Node::Link {
				url: link.url.clone(),
				text: match &link.text {
					Some(children) => Some(self.read_all(children, child_min, child_depth)?),
					None => None,
				},
				suppress_embed: link.suppress_embed,
			},
			MdNode::LineBreak => Node::LineBreak,
		};
		Some(Some(node))
	}
}

pub fn validate_ui_tree(tree: &UiTree) -> Result<(), &'static str> {
	if tree.nodes.len() > MAX_UI_NODES {
		return Err("ui tree has too many nodes");
	}
	if tree.roots.iter().any(|&root| root as usize >= tree.nodes.len()) {
		return Err("ui root index out of range");
	}
	for (index, node) in tree.nodes.iter().enumerate() {
		validate_ui_node(node, index, tree.nodes.len())?;
	}
	Ok(())
}

fn validate_ui_node(node: &UiNode, index: usize, count: usize) -> Result<(), &'static str> {
	match node {
		UiNode::Column(layout) | UiNode::Row(layout) => {
			let ordered = layout
				.children
				.iter()
				.all(|&child| (child as usize) > index && (child as usize) < count);
			if !ordered {
				return Err("ui child index must point forward and stay in range");
			}
			check_strings(layout.id.iter().chain([&layout.align]))
		}
		UiNode::Text(text) => check_strings([&text.value].into_iter().chain(text.style.color.iter())),
		UiNode::Button(button) => check_strings([&button.id, &button.label, &button.variant]),
		UiNode::Input(input) => check_strings([&input.id, &input.placeholder, &input.value]),
		UiNode::Toggle(toggle) => check_strings([&toggle.id, &toggle.label]),
		UiNode::Image(image) => {
			if !image.url.starts_with("https://") {
				return Err("ui image url must be https");
			}
			check_strings([&image.url])
		}
		UiNode::Badge(badge) => check_strings([&badge.label].into_iter().chain(badge.color.iter())),
		UiNode::Icon(name) => check_strings([name]),
		UiNode::Link(link) => {
			if !(link.url.starts_with("https://") || link.url.starts_with("http://")) {
				return Err("ui link url must be http or https");
			}
			check_strings([&link.label, &link.url])
		}
		UiNode::Divider | UiNode::Progress(_) => Ok(()),
	}
}

fn check_strings<'a>(strings: impl IntoIterator<Item = &'a String>) -> Result<(), &'static str> {
	if strings.into_iter().any(|value| value.len() > MAX_UI_STRING) {
		Err("ui string too long")
	} else {
		Ok(())
	}
}

pub fn user_view(user: &protocol::User) -> User {
	User {
		id: user.id.get(),
		username: user.username.clone(),
		display_name: user
			.global_name
			.clone()
			.filter(|name| !name.is_empty())
			.unwrap_or_else(|| user.username.clone()),
		avatar: user.avatar.clone(),
		bot: user.bot.unwrap_or(false),
	}
}

pub const DELETED_FLAG: u64 = 1 << 63;

pub fn message_view(message: &protocol::Message) -> MessageView {
	MessageView {
		id: message.id.get(),
		channel_id: message.channel_id.get(),
		guild_id: message.guild_id.map(protocol::Snowflake::get),
		author: user_view(&message.author),
		content: message.content.clone(),
		timestamp_ms: message
			.timestamp
			.unwrap_or_else(|| message.id.timestamp())
			.timestamp_millis(),
		edited: message.edited_timestamp.is_some(),
		reply_to: message
			.message_reference
			.as_ref()
			.and_then(|reference| reference.message_id)
			.map(protocol::Snowflake::get),
		flags: message.flags.bits(),
	}
}
