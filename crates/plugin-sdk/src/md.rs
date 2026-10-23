use crate::types::{MdCodeBlock, MdCommand, MdEmoji, MdHeading, MdLink, MdList, MdNode, MdTimestamp, MdTree};

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Md {
	Text(String),
	Bold(Vec<Md>),
	Italic(Vec<Md>),
	Underline(Vec<Md>),
	Strike(Vec<Md>),
	Spoiler(Vec<Md>),
	Code(String),
	CodeBlock {
		lang: Option<String>,
		code: String,
	},
	Quote(Vec<Md>),
	Heading {
		level: u8,
		children: Vec<Md>,
	},
	Subtext(Vec<Md>),
	List {
		ordered: bool,
		start: u32,
		items: Vec<Vec<Md>>,
	},
	UserMention(u64),
	RoleMention(u64),
	ChannelMention(u64),
	Everyone,
	Here,
	CustomEmoji {
		name: String,
		id: u64,
		animated: bool,
	},
	Timestamp {
		unix: i64,
		style: String,
	},
	CommandMention {
		name: String,
		id: u64,
	},
	GuildNav(String),
	Link {
		url: String,
		text: Option<Vec<Md>>,
		suppress_embed: bool,
	},
	LineBreak,
}

impl Md {
	pub fn text(value: impl Into<String>) -> Self {
		Self::Text(value.into())
	}

	pub fn subtext(value: impl Into<String>) -> Self {
		Self::Subtext(vec![Self::Text(value.into())])
	}

	pub fn plain_text(nodes: &[Md]) -> String {
		let mut out = String::new();
		collect_plain(nodes, &mut out);
		out
	}
}

fn collect_plain(nodes: &[Md], out: &mut String) {
	for node in nodes {
		match node {
			Md::Text(text) | Md::Code(text) => out.push_str(text),
			Md::CodeBlock { code, .. } => out.push_str(code),
			Md::LineBreak => out.push('\n'),
			Md::Bold(children)
			| Md::Italic(children)
			| Md::Underline(children)
			| Md::Strike(children)
			| Md::Spoiler(children)
			| Md::Quote(children)
			| Md::Subtext(children)
			| Md::Heading { children, .. } => collect_plain(children, out),
			Md::Link {
				text: Some(children), ..
			} => collect_plain(children, out),
			Md::List { items, .. } => {
				for item in items {
					collect_plain(item, out);
				}
			}
			_ => {}
		}
	}
}

pub fn from_tree(tree: &MdTree) -> Vec<Md> {
	read_all(&tree.nodes, &tree.roots, 0)
}

fn read_all(nodes: &[MdNode], indices: &[u32], depth: usize) -> Vec<Md> {
	if depth > MAX_DEPTH {
		return Vec::new();
	}
	indices
		.iter()
		.filter_map(|&index| nodes.get(index as usize))
		.map(|node| read(nodes, node, depth + 1))
		.collect()
}

fn read(nodes: &[MdNode], node: &MdNode, depth: usize) -> Md {
	match node {
		MdNode::Text(text) => Md::Text(text.clone()),
		MdNode::Bold(children) => Md::Bold(read_all(nodes, children, depth)),
		MdNode::Italic(children) => Md::Italic(read_all(nodes, children, depth)),
		MdNode::Underline(children) => Md::Underline(read_all(nodes, children, depth)),
		MdNode::Strike(children) => Md::Strike(read_all(nodes, children, depth)),
		MdNode::Spoiler(children) => Md::Spoiler(read_all(nodes, children, depth)),
		MdNode::Code(code) => Md::Code(code.clone()),
		MdNode::CodeBlock(block) => Md::CodeBlock {
			lang: block.lang.clone(),
			code: block.code.clone(),
		},
		MdNode::Quote(children) => Md::Quote(read_all(nodes, children, depth)),
		MdNode::Heading(heading) => Md::Heading {
			level: heading.level,
			children: read_all(nodes, &heading.children, depth),
		},
		MdNode::Subtext(children) => Md::Subtext(read_all(nodes, children, depth)),
		MdNode::List(list) => Md::List {
			ordered: list.ordered,
			start: list.start,
			items: list
				.items
				.iter()
				.map(|item| read_all(nodes, item, depth))
				.collect(),
		},
		MdNode::UserMention(id) => Md::UserMention(*id),
		MdNode::RoleMention(id) => Md::RoleMention(*id),
		MdNode::ChannelMention(id) => Md::ChannelMention(*id),
		MdNode::Everyone => Md::Everyone,
		MdNode::Here => Md::Here,
		MdNode::CustomEmoji(emoji) => Md::CustomEmoji {
			name: emoji.name.clone(),
			id: emoji.id,
			animated: emoji.animated,
		},
		MdNode::Timestamp(timestamp) => Md::Timestamp {
			unix: timestamp.unix,
			style: timestamp.style.clone(),
		},
		MdNode::CommandMention(command) => Md::CommandMention {
			name: command.name.clone(),
			id: command.id,
		},
		MdNode::GuildNav(name) => Md::GuildNav(name.clone()),
		MdNode::Link(link) => Md::Link {
			url: link.url.clone(),
			text: link
				.text
				.as_ref()
				.map(|children| read_all(nodes, children, depth)),
			suppress_embed: link.suppress_embed,
		},
		MdNode::LineBreak => Md::LineBreak,
	}
}

pub fn to_tree(nodes: &[Md]) -> MdTree {
	let mut flat = Vec::new();
	let roots = push_all(&mut flat, nodes);
	MdTree { nodes: flat, roots }
}

fn push_all(out: &mut Vec<MdNode>, nodes: &[Md]) -> Vec<u32> {
	nodes.iter().map(|node| push(out, node)).collect()
}

fn push(out: &mut Vec<MdNode>, node: &Md) -> u32 {
	let index = out.len();
	out.push(MdNode::LineBreak);
	let flat = match node {
		Md::Text(text) => MdNode::Text(text.clone()),
		Md::Bold(children) => MdNode::Bold(push_all(out, children)),
		Md::Italic(children) => MdNode::Italic(push_all(out, children)),
		Md::Underline(children) => MdNode::Underline(push_all(out, children)),
		Md::Strike(children) => MdNode::Strike(push_all(out, children)),
		Md::Spoiler(children) => MdNode::Spoiler(push_all(out, children)),
		Md::Code(code) => MdNode::Code(code.clone()),
		Md::CodeBlock { lang, code } => MdNode::CodeBlock(MdCodeBlock {
			lang: lang.clone(),
			code: code.clone(),
		}),
		Md::Quote(children) => MdNode::Quote(push_all(out, children)),
		Md::Heading { level, children } => MdNode::Heading(MdHeading {
			level: *level,
			children: push_all(out, children),
		}),
		Md::Subtext(children) => MdNode::Subtext(push_all(out, children)),
		Md::List {
			ordered,
			start,
			items,
		} => MdNode::List(MdList {
			ordered: *ordered,
			start: *start,
			items: items.iter().map(|item| push_all(out, item)).collect(),
		}),
		Md::UserMention(id) => MdNode::UserMention(*id),
		Md::RoleMention(id) => MdNode::RoleMention(*id),
		Md::ChannelMention(id) => MdNode::ChannelMention(*id),
		Md::Everyone => MdNode::Everyone,
		Md::Here => MdNode::Here,
		Md::CustomEmoji { name, id, animated } => MdNode::CustomEmoji(MdEmoji {
			name: name.clone(),
			id: *id,
			animated: *animated,
		}),
		Md::Timestamp { unix, style } => MdNode::Timestamp(MdTimestamp {
			unix: *unix,
			style: style.clone(),
		}),
		Md::CommandMention { name, id } => MdNode::CommandMention(MdCommand {
			name: name.clone(),
			id: *id,
		}),
		Md::GuildNav(name) => MdNode::GuildNav(name.clone()),
		Md::Link {
			url,
			text,
			suppress_embed,
		} => MdNode::Link(MdLink {
			url: url.clone(),
			text: text.as_deref().map(|children| push_all(out, children)),
			suppress_embed: *suppress_embed,
		}),
		Md::LineBreak => MdNode::LineBreak,
	};
	out[index] = flat;
	index as u32
}
