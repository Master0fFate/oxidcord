#[cfg(feature = "ext")]
use crate::model::{Color as ApiColor, InlineImage, RichNode, RichTree, StyledSpan};
use crate::types::{MdCodeBlock, MdCommand, MdEmoji, MdHeading, MdLink, MdList, MdNode, MdTimestamp, MdTree};

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Color {
	Token(String),
	Rgba(u32),
}

impl Color {
	pub fn token(name: impl Into<String>) -> Self {
		Self::Token(name.into())
	}

	pub fn rgb(red: u8, green: u8, blue: u8) -> Self {
		Self::Rgba(u32::from_be_bytes([red, green, blue, 255]))
	}
}

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
	InlineImage {
		url: String,
		alt: String,
		width: u32,
		height: u32,
	},
	Styled {
		children: Vec<Md>,
		color: Option<Color>,
		background: Option<Color>,
		mono: bool,
	},
}

impl Md {
	pub fn text(value: impl Into<String>) -> Self {
		Self::Text(value.into())
	}

	pub fn subtext(value: impl Into<String>) -> Self {
		Self::Subtext(vec![Self::Text(value.into())])
	}

	pub fn inline_image(url: impl Into<String>, alt: impl Into<String>) -> Self {
		Self::InlineImage {
			url: url.into(),
			alt: alt.into(),
			width: 0,
			height: 0,
		}
	}

	pub fn colored(children: Vec<Md>, color: Color) -> Self {
		Self::Styled {
			children,
			color: Some(color),
			background: None,
			mono: false,
		}
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
			Md::InlineImage { alt, .. } => out.push_str(alt),
			Md::Bold(children)
			| Md::Italic(children)
			| Md::Underline(children)
			| Md::Strike(children)
			| Md::Spoiler(children)
			| Md::Quote(children)
			| Md::Subtext(children)
			| Md::Styled { children, .. }
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

enum Item<'a> {
	Md(&'a MdNode),
	#[cfg(feature = "ext")]
	Image(&'a InlineImage),
	#[cfg(feature = "ext")]
	Styled(&'a StyledSpan),
}

trait Source {
	fn item(&self, index: usize) -> Option<Item<'_>>;
}

impl Source for [MdNode] {
	fn item(&self, index: usize) -> Option<Item<'_>> {
		self.get(index).map(Item::Md)
	}
}

#[cfg(feature = "ext")]
impl Source for [RichNode] {
	fn item(&self, index: usize) -> Option<Item<'_>> {
		Some(match self.get(index)? {
			RichNode::Md(node) => Item::Md(node),
			RichNode::InlineImage(image) => Item::Image(image),
			RichNode::Styled(span) => Item::Styled(span),
		})
	}
}

pub fn from_tree(tree: &MdTree) -> Vec<Md> {
	read_all(tree.nodes.as_slice(), &tree.roots, 0)
}

#[cfg(feature = "ext")]
pub fn from_rich(tree: &RichTree) -> Vec<Md> {
	read_all(tree.nodes.as_slice(), &tree.roots, 0)
}

fn read_all<S: Source + ?Sized>(nodes: &S, indices: &[u32], depth: usize) -> Vec<Md> {
	if depth > MAX_DEPTH {
		return Vec::new();
	}
	indices
		.iter()
		.filter_map(|&index| nodes.item(index as usize))
		.map(|item| read(nodes, item, depth + 1))
		.collect()
}

#[cfg(feature = "ext")]
fn sdk_color(color: &ApiColor) -> Color {
	match color {
		ApiColor::Token(token) => Color::Token(token.clone()),
		ApiColor::Rgba(rgba) => Color::Rgba(*rgba),
	}
}

#[cfg(feature = "ext")]
pub(crate) fn api_color(color: &Color) -> ApiColor {
	match color {
		Color::Token(token) => ApiColor::Token(token.clone()),
		Color::Rgba(rgba) => ApiColor::Rgba(*rgba),
	}
}

fn read<S: Source + ?Sized>(nodes: &S, item: Item<'_>, depth: usize) -> Md {
	let node = match item {
		Item::Md(node) => node,
		#[cfg(feature = "ext")]
		Item::Image(image) => {
			return Md::InlineImage {
				url: image.url.clone(),
				alt: image.alt.clone(),
				width: image.width,
				height: image.height,
			};
		}
		#[cfg(feature = "ext")]
		Item::Styled(span) => {
			return Md::Styled {
				children: read_all(nodes, &span.children, depth),
				color: span.color.as_ref().map(sdk_color),
				background: span.background.as_ref().map(sdk_color),
				mono: span.mono,
			};
		}
	};
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

enum Flat {
	Md(MdNode),
	#[cfg(feature = "ext")]
	Image(InlineImage),
	#[cfg(feature = "ext")]
	Styled(StyledSpan),
}

struct Flatten {
	out: Vec<Flat>,
	#[cfg(feature = "ext")]
	rich: bool,
}

pub fn to_tree(nodes: &[Md]) -> MdTree {
	let mut flatten = Flatten {
		out: Vec::new(),
		#[cfg(feature = "ext")]
		rich: false,
	};
	let roots = flatten.push_all(nodes);
	MdTree {
		nodes: flatten
			.out
			.into_iter()
			.map(|flat| match flat {
				Flat::Md(node) => node,
				#[cfg(feature = "ext")]
				Flat::Image(_) | Flat::Styled(_) => MdNode::LineBreak,
			})
			.collect(),
		roots,
	}
}

#[cfg(feature = "ext")]
pub fn to_rich(nodes: &[Md]) -> RichTree {
	let mut flatten = Flatten {
		out: Vec::new(),
		rich: true,
	};
	let roots = flatten.push_all(nodes);
	RichTree {
		nodes: flatten
			.out
			.into_iter()
			.map(|flat| match flat {
				Flat::Md(node) => RichNode::Md(node),
				Flat::Image(image) => RichNode::InlineImage(image),
				Flat::Styled(span) => RichNode::Styled(span),
			})
			.collect(),
		roots,
	}
}

impl Flatten {
	fn push_all(&mut self, nodes: &[Md]) -> Vec<u32> {
		nodes.iter().map(|node| self.push(node)).collect()
	}

	fn push(&mut self, node: &Md) -> u32 {
		let index = self.out.len();
		self.out.push(Flat::Md(MdNode::LineBreak));
		let flat = self.flat(node);
		self.out[index] = flat;
		index as u32
	}

	#[cfg(feature = "ext")]
	fn rich(&mut self, node: &Md) -> Option<Flat> {
		if !self.rich {
			return None;
		}
		Some(match node {
			Md::InlineImage {
				url,
				alt,
				width,
				height,
			} => Flat::Image(InlineImage {
				url: url.clone(),
				alt: alt.clone(),
				width: *width,
				height: *height,
			}),
			Md::Styled {
				children,
				color,
				background,
				mono,
			} => Flat::Styled(StyledSpan {
				children: self.push_all(children),
				color: color.as_ref().map(api_color),
				background: background.as_ref().map(api_color),
				mono: *mono,
			}),
			_ => return None,
		})
	}

	#[cfg(not(feature = "ext"))]
	fn rich(&mut self, _node: &Md) -> Option<Flat> {
		None
	}

	fn flat(&mut self, node: &Md) -> Flat {
		if let Some(flat) = self.rich(node) {
			return flat;
		}
		Flat::Md(match node {
			Md::Text(text) => MdNode::Text(text.clone()),
			Md::Bold(children) => MdNode::Bold(self.push_all(children)),
			Md::Italic(children) => MdNode::Italic(self.push_all(children)),
			Md::Underline(children) => MdNode::Underline(self.push_all(children)),
			Md::Strike(children) => MdNode::Strike(self.push_all(children)),
			Md::Spoiler(children) => MdNode::Spoiler(self.push_all(children)),
			Md::Code(code) => MdNode::Code(code.clone()),
			Md::CodeBlock { lang, code } => MdNode::CodeBlock(MdCodeBlock {
				lang: lang.clone(),
				code: code.clone(),
			}),
			Md::Quote(children) => MdNode::Quote(self.push_all(children)),
			Md::Heading { level, children } => MdNode::Heading(MdHeading {
				level: *level,
				children: self.push_all(children),
			}),
			Md::Subtext(children) => MdNode::Subtext(self.push_all(children)),
			Md::List {
				ordered,
				start,
				items,
			} => MdNode::List(MdList {
				ordered: *ordered,
				start: *start,
				items: items.iter().map(|item| self.push_all(item)).collect(),
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
				text: text.as_deref().map(|children| self.push_all(children)),
				suppress_embed: *suppress_embed,
			}),
			Md::LineBreak => MdNode::LineBreak,
			Md::InlineImage { alt, .. } => MdNode::Text(alt.clone()),
			Md::Styled { children, .. } => MdNode::Text(Md::plain_text(children)),
		})
	}
}
