#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
	Text(String),
	Bold(Vec<Node>),
	Italic(Vec<Node>),
	Underline(Vec<Node>),
	Strike(Vec<Node>),
	Spoiler(Vec<Node>),
	Code(String),
	CodeBlock {
		lang: Option<String>,
		code: String,
	},
	Quote(Vec<Node>),
	Heading {
		level: u8,
		children: Vec<Node>,
	},
	Subtext(Vec<Node>),
	List {
		ordered: bool,
		start: u32,
		items: Vec<Vec<Node>>,
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
		style: TimestampStyle,
	},
	CommandMention {
		name: String,
		id: u64,
	},
	GuildNav(GuildNavKind),
	Link {
		url: String,
		text: Option<Vec<Node>>,
		suppress_embed: bool,
	},
	LineBreak,
	InlineImage {
		src: String,
		alt: String,
		width: u32,
		height: u32,
	},
	Styled {
		children: Vec<Node>,
		color: Option<Color>,
		background: Option<Color>,
		mono: bool,
	},
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Color {
	Token(String),
	Rgba(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimestampStyle {
	ShortTime,
	LongTime,
	ShortDate,
	LongDate,
	#[default]
	ShortDateTime,
	LongDateTime,
	Relative,
}

impl TimestampStyle {
	pub(crate) fn from_code(code: u8) -> Option<Self> {
		match code {
			b't' => Some(Self::ShortTime),
			b'T' => Some(Self::LongTime),
			b'd' => Some(Self::ShortDate),
			b'D' => Some(Self::LongDate),
			b'f' => Some(Self::ShortDateTime),
			b'F' => Some(Self::LongDateTime),
			b'R' => Some(Self::Relative),
			_ => None,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuildNavKind {
	Customize,
	Browse,
	Guide,
	LinkedRoles,
}

impl Node {
	pub fn is_block(&self) -> bool {
		matches!(
			self,
			Node::CodeBlock { .. }
				| Node::Quote(_)
				| Node::Heading { .. }
				| Node::Subtext(_)
				| Node::List { .. }
		)
	}
}

pub(crate) fn for_each<'a>(nodes: &'a [Node], visit: &mut impl FnMut(&'a Node)) {
	for node in nodes {
		visit(node);
		match node {
			Node::Bold(children)
			| Node::Italic(children)
			| Node::Underline(children)
			| Node::Strike(children)
			| Node::Spoiler(children)
			| Node::Quote(children)
			| Node::Subtext(children)
			| Node::Heading { children, .. }
			| Node::Styled { children, .. }
			| Node::Link {
				text: Some(children), ..
			} => for_each(children, visit),
			Node::List { items, .. } => {
				for item in items {
					for_each(item, visit);
				}
			}
			_ => {}
		}
	}
}
