use markdown::{Color, GuildNavKind, Node, TimestampStyle};

use super::view::{ImagePolicy, sanitize_color};
use crate::api::model::{Color as ApiColor, InlineImage, RichNode, RichTree, StyledSpan};
use crate::api::{MdCodeBlock, MdCommand, MdEmoji, MdHeading, MdLink, MdList, MdNode, MdTimestamp, MdTree};

const MAX_MD_NODES: usize = 50_000;
const MAX_MD_DEPTH: usize = 64;
const INLINE_IMAGE_DEFAULT: u32 = 22;
const INLINE_IMAGE_MAX: u32 = 160;

pub fn md_to_tree(nodes: &[Node]) -> MdTree {
	let mut flatten = Flatten {
		out: Vec::new(),
		legacy: true,
	};
	let roots = flatten.push_all(nodes);
	MdTree {
		nodes: flatten
			.out
			.into_iter()
			.map(|node| match node {
				RichNode::Md(node) => node,
				RichNode::InlineImage(image) => MdNode::Text(image.alt),
				RichNode::Styled(_) => MdNode::LineBreak,
			})
			.collect(),
		roots,
	}
}

pub fn md_to_rich(nodes: &[Node]) -> RichTree {
	let mut flatten = Flatten {
		out: Vec::new(),
		legacy: false,
	};
	let roots = flatten.push_all(nodes);
	RichTree {
		nodes: flatten.out,
		roots,
	}
}

struct Flatten {
	out: Vec<RichNode>,
	legacy: bool,
}

impl Flatten {
	fn push_all(&mut self, nodes: &[Node]) -> Vec<u32> {
		nodes.iter().map(|node| self.push(node)).collect()
	}

	fn push(&mut self, node: &Node) -> u32 {
		let index = self.out.len();
		self.out.push(RichNode::Md(MdNode::LineBreak));
		let flat = match node {
			Node::InlineImage {
				src,
				alt,
				width,
				height,
			} if !self.legacy => RichNode::InlineImage(InlineImage {
				url: src.clone(),
				alt: alt.clone(),
				width: *width,
				height: *height,
			}),
			Node::InlineImage { alt, .. } => RichNode::Md(MdNode::Text(alt.clone())),
			Node::Styled {
				children,
				color,
				background,
				mono,
			} if !self.legacy => RichNode::Styled(StyledSpan {
				children: self.push_all(children),
				color: color.as_ref().map(api_color),
				background: background.as_ref().map(api_color),
				mono: *mono,
			}),
			Node::Styled { children, .. } => RichNode::Md(MdNode::Text(
				markdown::Document {
					nodes: children.clone(),
				}
				.plain_text(),
			)),
			other => RichNode::Md(self.md_node(other)),
		};
		self.out[index] = flat;
		index as u32
	}

	fn md_node(&mut self, node: &Node) -> MdNode {
		match node {
			Node::Text(text) => MdNode::Text(text.clone()),
			Node::Bold(children) => MdNode::Bold(self.push_all(children)),
			Node::Italic(children) => MdNode::Italic(self.push_all(children)),
			Node::Underline(children) => MdNode::Underline(self.push_all(children)),
			Node::Strike(children) => MdNode::Strike(self.push_all(children)),
			Node::Spoiler(children) => MdNode::Spoiler(self.push_all(children)),
			Node::Code(code) => MdNode::Code(code.clone()),
			Node::CodeBlock { lang, code } => MdNode::CodeBlock(MdCodeBlock {
				lang: lang.clone(),
				code: code.clone(),
			}),
			Node::Quote(children) => MdNode::Quote(self.push_all(children)),
			Node::Heading { level, children } => MdNode::Heading(MdHeading {
				level: *level,
				children: self.push_all(children),
			}),
			Node::Subtext(children) => MdNode::Subtext(self.push_all(children)),
			Node::List {
				ordered,
				start,
				items,
			} => MdNode::List(MdList {
				ordered: *ordered,
				start: *start,
				items: items.iter().map(|item| self.push_all(item)).collect(),
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
				text: text.as_deref().map(|children| self.push_all(children)),
				suppress_embed: *suppress_embed,
			}),
			Node::LineBreak | Node::InlineImage { .. } | Node::Styled { .. } => MdNode::LineBreak,
		}
	}
}

fn api_color(color: &Color) -> ApiColor {
	match color {
		Color::Token(token) => ApiColor::Token(token.clone()),
		Color::Rgba(rgba) => ApiColor::Rgba(*rgba),
	}
}

fn md_color(color: &ApiColor) -> Option<Color> {
	match sanitize_color(color)? {
		ApiColor::Token(token) => Some(Color::Token(token)),
		ApiColor::Rgba(rgba) => Some(Color::Rgba(rgba)),
	}
}

pub(crate) fn style_code(style: TimestampStyle) -> &'static str {
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

pub(crate) fn style_from_code(code: &str) -> TimestampStyle {
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

enum Item<'a> {
	Md(&'a MdNode),
	Image(&'a InlineImage),
	Styled(&'a StyledSpan),
}

trait Source {
	fn count(&self) -> usize;
	fn item(&self, index: usize) -> Item<'_>;
}

impl Source for [MdNode] {
	fn count(&self) -> usize {
		self.len()
	}

	fn item(&self, index: usize) -> Item<'_> {
		Item::Md(&self[index])
	}
}

impl Source for [RichNode] {
	fn count(&self) -> usize {
		self.len()
	}

	fn item(&self, index: usize) -> Item<'_> {
		match &self[index] {
			RichNode::Md(node) => Item::Md(node),
			RichNode::InlineImage(image) => Item::Image(image),
			RichNode::Styled(span) => Item::Styled(span),
		}
	}
}

pub fn tree_to_md(tree: &MdTree) -> Option<Vec<Node>> {
	Reader {
		nodes: tree.nodes.as_slice(),
		budget: MAX_MD_NODES,
		images: None,
	}
	.read_all(&tree.roots, 0, 0)
}

pub fn rich_to_md(tree: &RichTree, images: &ImagePolicy) -> Vec<Node> {
	Reader {
		nodes: tree.nodes.as_slice(),
		budget: MAX_MD_NODES,
		images: Some(images),
	}
	.read_all(&tree.roots, 0, 0)
	.unwrap_or_default()
}

struct Reader<'a, S: Source + ?Sized> {
	nodes: &'a S,
	budget: usize,
	images: Option<&'a ImagePolicy>,
}

impl<S: Source + ?Sized> Reader<'_, S> {
	fn lenient(&self) -> bool {
		self.images.is_some()
	}

	fn read_all(&mut self, indices: &[u32], min_index: usize, depth: usize) -> Option<Vec<Node>> {
		if depth > MAX_MD_DEPTH {
			return self.lenient().then(Vec::new);
		}
		let mut out = Vec::with_capacity(indices.len());
		for &index in indices {
			let index = index as usize;
			if index < min_index || index >= self.nodes.count() || self.budget == 0 {
				if self.lenient() {
					continue;
				}
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
		let node = match self.nodes.item(index) {
			Item::Image(image) => return Some(self.image(image)),
			Item::Styled(span) => Node::Styled {
				children: self.read_all(&span.children, child_min, child_depth)?,
				color: span.color.as_ref().and_then(md_color),
				background: span.background.as_ref().and_then(md_color),
				mono: span.mono,
			},
			Item::Md(node) => match node {
				MdNode::Text(text) => Node::Text(text.clone()),
				MdNode::Bold(children) => Node::Bold(self.read_all(children, child_min, child_depth)?),
				MdNode::Italic(children) => Node::Italic(self.read_all(children, child_min, child_depth)?),
				MdNode::Underline(children) => {
					Node::Underline(self.read_all(children, child_min, child_depth)?)
				}
				MdNode::Strike(children) => Node::Strike(self.read_all(children, child_min, child_depth)?),
				MdNode::Spoiler(children) => {
					Node::Spoiler(self.read_all(children, child_min, child_depth)?)
				}
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
				MdNode::Subtext(children) => {
					Node::Subtext(self.read_all(children, child_min, child_depth)?)
				}
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
			},
		};
		Some(Some(node))
	}

	fn image(&self, image: &InlineImage) -> Option<Node> {
		let policy = self.images?;
		if !policy.allows(&image.url) {
			return None;
		}
		let size = |value: u32| match value {
			0 => INLINE_IMAGE_DEFAULT,
			value => value.min(INLINE_IMAGE_MAX),
		};
		Some(Node::InlineImage {
			src: image.url.clone(),
			alt: super::view::clip(&image.alt, 128),
			width: size(image.width),
			height: size(image.height),
		})
	}
}
