use crate::ast::Node;
use crate::entity;
use crate::fence::{self, Fence};
use crate::link;

const MAX_DEPTH: u8 = 12;

const BOLD: usize = 0;
const UNDERLINE: usize = 1;
const ITALIC_STAR: usize = 2;
const ITALIC_UNDERSCORE: usize = 3;
const STRIKE: usize = 4;
const SPOILER: usize = 5;
const TICK_ONE: usize = 6;
const KINDS: usize = 8;

enum Step {
	Literal(usize),
	Emit(Node, usize),
}

type Span = Option<(Vec<Node>, usize)>;

pub(crate) fn parse(src: &str) -> Vec<Node> {
	Parser::new(src, 0, false).run()
}

struct Parser<'a> {
	src: &'a str,
	bytes: &'a [u8],
	depth: u8,
	in_link: bool,
	misses: [usize; KINDS],
	nodes: Vec<Node>,
	text: String,
}

impl<'a> Parser<'a> {
	fn new(src: &'a str, depth: u8, in_link: bool) -> Self {
		Self {
			src,
			bytes: src.as_bytes(),
			depth,
			in_link,
			misses: [usize::MAX; KINDS],
			nodes: Vec::new(),
			text: String::new(),
		}
	}

	fn child(&self, content: &'a str) -> Vec<Node> {
		Parser::new(content, self.depth + 1, self.in_link).run()
	}

	fn link_text(&self, content: &'a str) -> Vec<Node> {
		Parser::new(content, self.depth + 1, true).run()
	}

	fn flush_text(&mut self) {
		if !self.text.is_empty() {
			self.nodes.push(Node::Text(std::mem::take(&mut self.text)));
		}
	}

	fn run(mut self) -> Vec<Node> {
		let src = self.src;
		if self.depth >= MAX_DEPTH {
			return if src.is_empty() {
				Vec::new()
			} else {
				vec![Node::Text(src.to_owned())]
			};
		}
		let mut raw_start = 0;
		let mut at = 0;
		while at < self.bytes.len() {
			let step = match self.bytes[at] {
				b'\\' => {
					let escaped = src[at + 1..].chars().next();
					match escaped {
						Some(c) if !c.is_ascii_alphanumeric() && !c.is_whitespace() => {
							self.text.push_str(&src[raw_start..at]);
							self.text.push(c);
							at += 1 + c.len_utf8();
							raw_start = at;
							continue;
						}
						_ => Step::Literal(at + 1),
					}
				}
				b'\r' => self.carriage_return(at),
				b'\n' => Step::Emit(Node::LineBreak, at + 1),
				b'*' => self.star(at),
				b'_' => self.underscore(at),
				b'~' => self.pair(at, b'~', STRIKE, false, Node::Strike),
				b'|' => self.pair(at, b'|', SPOILER, false, Node::Spoiler),
				b'`' => self.backtick(at),
				b'<' => match entity::parse(src, at, !self.in_link) {
					Some((node, end)) => Step::Emit(node, end),
					None => Step::Literal(at + 1),
				},
				b'@' => self.keyword_mention(at),
				b'h' if !self.in_link => self.bare_link(at),
				b'[' if !self.in_link => self.masked_link(at),
				_ => Step::Literal(at + 1),
			};
			match step {
				Step::Literal(next) => at = next,
				Step::Emit(node, end) => {
					self.text.push_str(&src[raw_start..at]);
					self.flush_text();
					self.nodes.push(node);
					at = end;
					raw_start = end;
				}
			}
		}
		self.text.push_str(&src[raw_start..]);
		self.flush_text();
		self.nodes
	}

	fn carriage_return(&self, at: usize) -> Step {
		if self.bytes.get(at + 1) == Some(&b'\n') {
			Step::Emit(Node::LineBreak, at + 2)
		} else {
			Step::Literal(at + 1)
		}
	}

	fn pair(&mut self, at: usize, ch: u8, kind: usize, lookahead: bool, wrap: fn(Vec<Node>) -> Node) -> Step {
		if self.bytes.get(at + 1) != Some(&ch) {
			return Step::Literal(at + 1);
		}
		match self.closing_pair(at, ch, kind, lookahead) {
			Some((children, end)) => Step::Emit(wrap(children), end),
			None => Step::Literal(at + 1),
		}
	}

	fn closing_pair(&mut self, at: usize, ch: u8, kind: usize, lookahead: bool) -> Span {
		if at >= self.misses[kind] {
			return None;
		}
		let bytes = self.bytes;
		let start = at + 2;
		let mut j = start;
		while j + 1 < bytes.len() {
			if bytes[j] == b'\\' {
				j += 2;
			} else if j > start
				&& bytes[j] == ch
				&& bytes[j + 1] == ch
				&& !(lookahead && bytes.get(j + 2) == Some(&ch))
			{
				return Some((self.child(&self.src[start..j]), j + 2));
			} else {
				j += 1;
			}
		}
		self.misses[kind] = at;
		None
	}

	fn star(&mut self, at: usize) -> Step {
		if self.bytes.get(at + 1) == Some(&b'*')
			&& let Some((children, end)) = self.closing_pair(at, b'*', BOLD, true)
		{
			return Step::Emit(Node::Bold(children), end);
		}
		match self.italic_star(at) {
			Some((children, end)) => Step::Emit(Node::Italic(children), end),
			None => Step::Literal(at + 1),
		}
	}

	fn italic_star(&mut self, at: usize) -> Span {
		if at >= self.misses[ITALIC_STAR] {
			return None;
		}
		let bytes = self.bytes;
		let next = *bytes.get(at + 1)?;
		if next.is_ascii_whitespace() || (next == b'*' && bytes.get(at + 2) != Some(&b'*')) {
			return None;
		}
		let mut j = at + 1;
		while j < bytes.len() {
			match bytes[j] {
				b'\\' => j += 2,
				b'*' if bytes.get(j + 1) == Some(&b'*') => j += 2,
				b'*' => {
					if self.src[..j].chars().next_back().is_some_and(char::is_whitespace) {
						return None;
					}
					return Some((self.child(&self.src[at + 1..j]), j + 1));
				}
				_ => j += 1,
			}
		}
		self.misses[ITALIC_STAR] = at;
		None
	}

	fn underscore(&mut self, at: usize) -> Step {
		if self.bytes.get(at + 1) == Some(&b'_')
			&& let Some((children, end)) = self.closing_pair(at, b'_', UNDERLINE, true)
		{
			return Step::Emit(Node::Underline(children), end);
		}
		match self.italic_underscore(at) {
			Some((children, end)) => Step::Emit(Node::Italic(children), end),
			None => Step::Literal(at + 1),
		}
	}

	fn italic_underscore(&mut self, at: usize) -> Span {
		if at >= self.misses[ITALIC_UNDERSCORE] {
			return None;
		}
		let bytes = self.bytes;
		if at > 0 && is_word(bytes[at - 1]) {
			return None;
		}
		let mut j = at + 1;
		while j < bytes.len() {
			match bytes[j] {
				b'\\' => j += 2,
				b'_' if bytes.get(j + 1) == Some(&b'_') => j += 2,
				b'_' => {
					if j == at + 1 || bytes.get(j + 1).is_some_and(|&b| is_word(b)) {
						return None;
					}
					return Some((self.child(&self.src[at + 1..j]), j + 1));
				}
				_ => j += 1,
			}
		}
		self.misses[ITALIC_UNDERSCORE] = at;
		None
	}

	fn backtick(&mut self, at: usize) -> Step {
		let run = self.bytes[at..].iter().take_while(|&&b| b == b'`').count();
		match run {
			1 | 2 => self.code_span(at, run),
			3 => match fence::parse(self.src, at) {
				Fence::Block { lang, code, end } => {
					let node = Node::CodeBlock {
						lang: lang.map(str::to_owned),
						code: code.to_owned(),
					};
					Step::Emit(node, fence::skip_newline(self.src, end))
				}
				_ => Step::Literal(at + 3),
			},
			_ => Step::Literal(at + run),
		}
	}

	fn code_span(&mut self, at: usize, run: usize) -> Step {
		let kind = TICK_ONE + run - 1;
		if at >= self.misses[kind] {
			return Step::Literal(at + run);
		}
		let bytes = self.bytes;
		let start = at + run;
		let mut j = start;
		while let Some(offset) = bytes[j..].iter().position(|&b| b == b'`') {
			let tick = j + offset;
			let end = tick + bytes[tick..].iter().take_while(|&&b| b == b'`').count();
			if end - tick == run {
				return Step::Emit(Node::Code(strip_padding(&self.src[start..tick]).to_owned()), end);
			}
			j = end;
		}
		self.misses[kind] = at;
		Step::Literal(at + run)
	}

	fn keyword_mention(&self, at: usize) -> Step {
		let rest = &self.src[at + 1..];
		let (node, length) = if rest.starts_with("everyone") {
			(Node::Everyone, 8)
		} else if rest.starts_with("here") {
			(Node::Here, 4)
		} else {
			return Step::Literal(at + 1);
		};
		let end = at + 1 + length;
		let boundary_before = at == 0 || !self.bytes[at - 1].is_ascii_alphanumeric();
		let boundary_after = self.bytes.get(end).is_none_or(|&b| !is_word(b));
		if boundary_before && boundary_after {
			Step::Emit(node, end)
		} else {
			Step::Literal(at + 1)
		}
	}

	fn bare_link(&self, at: usize) -> Step {
		match link::bare(self.src, at) {
			Some(end) => {
				let url = self.src[at..end].to_owned();
				Step::Emit(
					Node::Link {
						url,
						text: None,
						suppress_embed: false,
					},
					end,
				)
			}
			None => Step::Literal(at + 1),
		}
	}

	fn masked_link(&self, at: usize) -> Step {
		match link::masked(self.src, at) {
			Some(masked) => {
				let link = Node::Link {
					url: masked.url.to_owned(),
					text: Some(self.link_text(masked.text)),
					suppress_embed: masked.suppress_embed,
				};
				Step::Emit(link, masked.end)
			}
			None => Step::Literal(at + 1),
		}
	}
}

fn is_word(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_'
}

fn strip_padding(content: &str) -> &str {
	let mut text = content;
	if text.starts_with(' ') && text.trim_start_matches(' ').starts_with('`') {
		text = &text[1..];
	}
	if text.ends_with(' ') && text.trim_end_matches(' ').ends_with('`') {
		text = &text[..text.len() - 1];
	}
	text
}
