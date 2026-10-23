use crate::ast::Node;
use crate::fence::{self, Fence};
use crate::inline;

const MAX_LIST_LEVEL: u8 = 2;

struct ListLine<'a> {
	level: u8,
	ordered: bool,
	number: u32,
	text: &'a str,
}

struct Blocks<'a> {
	input: &'a str,
	nested: bool,
	nodes: Vec<Node>,
	run_start: usize,
}

pub(crate) fn parse(input: &str) -> Vec<Node> {
	Blocks::new(input, false).run()
}

impl<'a> Blocks<'a> {
	fn new(input: &'a str, nested: bool) -> Self {
		Self {
			input,
			nested,
			nodes: Vec::new(),
			run_start: 0,
		}
	}

	fn run(mut self) -> Vec<Node> {
		let input = self.input;
		let len = input.len();
		let mut pos = 0;
		while pos < len {
			let end = line_end(input, pos);
			let line = trim_carriage_return(&input[pos..end]);
			pos = match self.block(pos, end, line) {
				Some(after) => {
					self.run_start = after;
					after
				}
				None => (skip_fences(input, pos, end) + 1).min(len),
			};
		}
		let tail = &input[self.run_start..];
		if !tail.is_empty() {
			self.nodes.extend(inline::parse(tail));
		}
		self.nodes
	}

	fn flush(&mut self, pos: usize) {
		let run = &self.input[self.run_start..pos];
		let run = run.strip_suffix('\n').unwrap_or(run);
		let run = run.strip_suffix('\r').unwrap_or(run);
		if !run.is_empty() {
			self.nodes.extend(inline::parse(run));
		}
	}

	fn push(&mut self, pos: usize, node: Node) {
		self.flush(pos);
		self.nodes.push(node);
	}

	fn block(&mut self, pos: usize, end: usize, line: &'a str) -> Option<usize> {
		let input = self.input;
		let next = (end + 1).min(input.len());
		let trimmed = line.trim_start_matches(' ');
		if !self.nested {
			if let Some(first_line) = trimmed.strip_prefix(">>> ") {
				let content_start = pos + (line.len() - trimmed.len()) + 4;
				let content = &input[content_start..];
				if !first_line.trim().is_empty() || !content.trim().is_empty() {
					let children = Blocks::new(content, true).run();
					self.push(pos, Node::Quote(children));
					return Some(input.len());
				}
			}
			if let Some(rest) = trimmed.strip_prefix("> ")
				&& !rest.trim().is_empty()
			{
				self.push(pos, Node::Quote(inline::parse(rest)));
				return Some(next);
			}
		}
		if let Some((level, rest)) = heading(trimmed) {
			self.push(
				pos,
				Node::Heading {
					level,
					children: inline::parse(rest),
				},
			);
			return Some(next);
		}
		if let Some(rest) = trimmed.strip_prefix("-# ")
			&& !rest.trim().is_empty()
		{
			self.push(pos, Node::Subtext(inline::parse(rest)));
			return Some(next);
		}
		list_line(line)?;
		let (lines, after) = collect_list(input, pos);
		self.flush(pos);
		let mut index = 0;
		while index < lines.len() {
			self.nodes.push(build_list(&lines, &mut index, 0));
		}
		Some(after)
	}
}

fn line_end(input: &str, pos: usize) -> usize {
	input.as_bytes()[pos..]
		.iter()
		.position(|&b| b == b'\n')
		.map_or(input.len(), |offset| pos + offset)
}

fn trim_carriage_return(line: &str) -> &str {
	line.strip_suffix('\r').unwrap_or(line)
}

fn heading(trimmed: &str) -> Option<(u8, &str)> {
	let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
	if !(1..=3).contains(&hashes) {
		return None;
	}
	let rest = trimmed[hashes..].strip_prefix(' ')?.trim();
	(!rest.is_empty()).then_some((hashes as u8, rest))
}

fn skip_fences(input: &str, pos: usize, line_end_pos: usize) -> usize {
	let bytes = input.as_bytes();
	let mut end = line_end_pos;
	let mut from = pos;
	while let Some(open) = find_fence_open(bytes, from, end) {
		match fence::parse(input, open) {
			Fence::Block { end: close_end, .. } => {
				end = line_end(input, close_end);
				from = close_end;
			}
			_ => break,
		}
	}
	end
}

fn find_fence_open(bytes: &[u8], from: usize, to: usize) -> Option<usize> {
	let mut at = from;
	while at < to {
		let start = at + bytes[at..to].iter().position(|&b| b == b'`')?;
		let run = bytes[start..to].iter().take_while(|&&b| b == b'`').count();
		if run == 3 {
			return Some(start);
		}
		at = start + run;
	}
	None
}

fn list_line(line: &str) -> Option<ListLine<'_>> {
	let indent_len = line.bytes().take_while(|&b| b == b' ' || b == b'\t').count();
	let (indent, rest) = line.split_at(indent_len);
	let tabs = indent.bytes().filter(|&b| b == b'\t').count();
	let level = ((tabs + (indent_len - tabs) / 2) as u8).min(MAX_LIST_LEVEL);
	let (ordered, number, text) = match rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
		Some(text) => (false, 1, text),
		None => {
			let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
			if !(1..=9).contains(&digits) {
				return None;
			}
			let text = rest[digits..].strip_prefix(". ")?;
			(true, rest[..digits].parse().ok()?, text)
		}
	};
	let text = text.trim();
	(!text.is_empty()).then_some(ListLine {
		level,
		ordered,
		number,
		text,
	})
}

fn collect_list(input: &str, pos: usize) -> (Vec<ListLine<'_>>, usize) {
	let mut lines = Vec::new();
	let mut cursor = pos;
	while cursor < input.len() {
		let end = line_end(input, cursor);
		let Some(line) = list_line(trim_carriage_return(&input[cursor..end])) else {
			break;
		};
		lines.push(line);
		cursor = (end + 1).min(input.len());
	}
	(lines, cursor)
}

fn build_list(lines: &[ListLine], index: &mut usize, level: u8) -> Node {
	let first = &lines[*index];
	let (ordered, start) = (first.ordered, first.number);
	let mut items: Vec<Vec<Node>> = Vec::new();
	while let Some(line) = lines.get(*index) {
		if line.level < level {
			break;
		}
		if line.level > level
			&& let Some(last) = items.last_mut()
		{
			last.push(build_list(lines, index, level + 1));
			continue;
		}
		if line.ordered != ordered && !items.is_empty() {
			break;
		}
		items.push(inline::parse(line.text));
		*index += 1;
	}
	Node::List {
		ordered,
		start,
		items,
	}
}
