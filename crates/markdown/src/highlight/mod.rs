mod languages;

use languages::{Language, Special};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
	Keyword,
	String,
	Comment,
	Number,
	Punctuation,
	Identifier,
	Type,
	Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
	pub start: usize,
	pub end: usize,
	pub kind: TokenKind,
}

pub fn highlight(language: &str, code: &str) -> Vec<Span> {
	if code.is_empty() {
		return Vec::new();
	}
	match languages::find(language) {
		Some(language) => Tokenizer {
			language,
			code,
			bytes: code.as_bytes(),
			spans: Vec::new(),
		}
		.run(),
		None => vec![Span {
			start: 0,
			end: code.len(),
			kind: TokenKind::Plain,
		}],
	}
}

struct Tokenizer<'a> {
	language: &'static Language,
	code: &'a str,
	bytes: &'a [u8],
	spans: Vec<Span>,
}

impl Tokenizer<'_> {
	fn run(mut self) -> Vec<Span> {
		let mut pos = 0;
		while let Some(c) = self.code[pos..].chars().next() {
			let (end, kind) = self.token(pos, c);
			self.push(pos, end, kind);
			pos = end;
		}
		self.spans
	}

	fn push(&mut self, start: usize, end: usize, kind: TokenKind) {
		match self.spans.last_mut() {
			Some(last) if last.kind == kind && last.end == start => last.end = end,
			_ => self.spans.push(Span { start, end, kind }),
		}
	}

	fn token(&self, pos: usize, c: char) -> (usize, TokenKind) {
		let language = self.language;
		if c.is_whitespace() {
			return (self.take_while(pos, char::is_whitespace), TokenKind::Plain);
		}
		if let Some(token) = self.line_start_token(pos, c) {
			return token;
		}
		if let Some(end) = self.comment(pos) {
			return (end, TokenKind::Comment);
		}
		if c.is_ascii() && language.quotes.contains(&(c as u8)) {
			return self.quoted(pos, c as u8);
		}
		if c.is_ascii_digit() {
			return (self.number_end(pos), TokenKind::Number);
		}
		if language.special == Special::Css
			&& let Some(token) = self.css_token(pos, c)
		{
			return token;
		}
		if self.starts_identifier(pos, c) {
			return self.word(pos);
		}
		let end = pos + c.len_utf8();
		if c.is_ascii_punctuation() {
			(end, TokenKind::Punctuation)
		} else {
			(end, TokenKind::Plain)
		}
	}

	fn take_while(&self, pos: usize, keep: impl Fn(char) -> bool) -> usize {
		self.code[pos..]
			.find(|c: char| !keep(c))
			.map_or(self.code.len(), |offset| pos + offset)
	}

	fn line_end(&self, pos: usize) -> usize {
		self.bytes[pos..]
			.iter()
			.position(|&b| b == b'\n')
			.map_or(self.bytes.len(), |offset| pos + offset)
	}

	fn at_line_start(&self, pos: usize) -> bool {
		self.bytes[..pos]
			.iter()
			.rev()
			.find(|&&b| b != b' ' && b != b'\t')
			.is_none_or(|&b| b == b'\n')
	}

	fn line_start_token(&self, pos: usize, c: char) -> Option<(usize, TokenKind)> {
		let rest = &self.code[pos..];
		match (self.language.special, c) {
			(Special::Markdown, '#') if self.at_line_start(pos) => {
				Some((self.line_end(pos), TokenKind::Keyword))
			}
			(Special::Markdown, '`') if rest.starts_with("```") && self.at_line_start(pos) => {
				Some((self.line_end(pos), TokenKind::Comment))
			}
			(Special::C, '#') if self.at_line_start(pos) => {
				let after_hash = self.take_while(pos + 1, |c| c == ' ' || c == '\t');
				Some((
					self.take_while(after_hash, char::is_alphanumeric),
					TokenKind::Keyword,
				))
			}
			(Special::Toml, '[') if self.at_line_start(pos) => {
				let line_end = self.line_end(pos);
				let mut end = self.bytes[pos..line_end]
					.iter()
					.position(|&b| b == b']')
					.map_or(line_end, |offset| pos + offset + 1);
				if end < line_end && self.bytes[end] == b']' {
					end += 1;
				}
				Some((end, TokenKind::Type))
			}
			_ => None,
		}
	}

	fn comment(&self, pos: usize) -> Option<usize> {
		let rest = &self.code[pos..];
		if self
			.language
			.line_comments
			.iter()
			.any(|prefix| rest.starts_with(prefix))
		{
			return Some(self.line_end(pos));
		}
		let (open, close) = self.language.block_comment?;
		if !rest.starts_with(open) {
			return None;
		}
		let from = pos + open.len();
		Some(
			self.code[from..]
				.find(close)
				.map_or(self.code.len(), |offset| from + offset + close.len()),
		)
	}

	fn quoted(&self, pos: usize, quote: u8) -> (usize, TokenKind) {
		let language = self.language;
		if language.special == Special::Rust && quote == b'\'' {
			return self.rust_quote(pos);
		}
		if language.triple_quotes && self.bytes[pos..].starts_with(&[quote; 3]) {
			let closing = if quote == b'"' { "\"\"\"" } else { "'''" };
			let from = pos + 3;
			let end = self.code[from..]
				.find(closing)
				.map_or(self.code.len(), |offset| from + offset + 3);
			return (end, TokenKind::String);
		}
		let multiline = quote == b'`' && language.special != Special::Markdown;
		(self.string_end(pos, quote, multiline), TokenKind::String)
	}

	fn string_end(&self, pos: usize, quote: u8, multiline: bool) -> usize {
		let mut at = pos + 1;
		while at < self.bytes.len() {
			match self.bytes[at] {
				b'\\' => at += 2,
				b if b == quote => return at + 1,
				b'\n' if !multiline => return at,
				_ => at += 1,
			}
		}
		self.bytes.len()
	}

	fn rust_quote(&self, pos: usize) -> (usize, TokenKind) {
		let mut chars = self.code[pos + 1..].chars();
		let is_char_literal = match chars.next() {
			Some('\\') => true,
			Some(_) => chars.next() == Some('\''),
			None => false,
		};
		if is_char_literal {
			(self.string_end(pos, b'\'', false), TokenKind::String)
		} else {
			(
				self.take_while(pos + 1, |c| c.is_alphanumeric() || c == '_'),
				TokenKind::Type,
			)
		}
	}

	fn number_end(&self, pos: usize) -> usize {
		let mut at = pos;
		while let Some(&b) = self.bytes.get(at) {
			let continues = b.is_ascii_alphanumeric()
				|| b == b'_' || (b == b'.'
				&& self.bytes.get(at + 1).is_some_and(u8::is_ascii_digit));
			if !continues {
				break;
			}
			at += 1;
		}
		at
	}

	fn css_token(&self, pos: usize, c: char) -> Option<(usize, TokenKind)> {
		match c {
			'#' => {
				let end = self.take_while(pos + 1, |c| c.is_ascii_alphanumeric());
				let is_color = matches!(end - pos - 1, 3 | 4 | 6 | 8)
					&& self.code[pos + 1..end].bytes().all(|b| b.is_ascii_hexdigit());
				is_color.then_some((end, TokenKind::Number))
			}
			'@' => {
				let end = self.identifier_end(pos + 1);
				(end > pos + 1).then_some((end, TokenKind::Keyword))
			}
			_ => None,
		}
	}

	fn starts_identifier(&self, pos: usize, c: char) -> bool {
		let language = self.language;
		c.is_alphabetic()
			|| c == '_'
			|| (c == '$' && language.dollar_identifiers)
			|| (c == '-'
				&& language.dash_identifiers
				&& self.code[pos + 1..].starts_with(char::is_alphabetic))
	}

	fn identifier_end(&self, pos: usize) -> usize {
		let language = self.language;
		let mut chars = self.code[pos..].char_indices().peekable();
		let mut end = pos;
		while let Some((offset, c)) = chars.next() {
			let part = c.is_alphanumeric()
				|| c == '_' || (c == '$' && language.dollar_identifiers)
				|| (c == '-'
					&& language.dash_identifiers
					&& chars
						.peek()
						.is_some_and(|&(_, next)| next.is_alphanumeric() || next == '_'));
			if !part {
				break;
			}
			end = pos + offset + c.len_utf8();
		}
		end
	}

	fn word(&self, pos: usize) -> (usize, TokenKind) {
		let language = self.language;
		let end = self.identifier_end(pos);
		let word = &self.code[pos..end];
		if language.special == Special::Rust
			&& matches!(word, "r" | "br")
			&& let Some(raw_end) = self.raw_string_end(end)
		{
			return (raw_end, TokenKind::String);
		}
		let is_in = |list: &[&str]| {
			if language.case_insensitive {
				list.iter().any(|known| known.eq_ignore_ascii_case(word))
			} else {
				list.contains(&word)
			}
		};
		let kind = if is_in(language.keywords) || self.is_tag_name(pos) {
			TokenKind::Keyword
		} else if is_in(language.types)
			|| (language.capitalized_types && word.chars().next().is_some_and(char::is_uppercase))
		{
			TokenKind::Type
		} else {
			TokenKind::Identifier
		};
		(end, kind)
	}

	fn is_tag_name(&self, pos: usize) -> bool {
		let before = &self.bytes[..pos];
		self.language.special == Special::Html
			&& (before.ends_with(b"<") || before.ends_with(b"</") || before.ends_with(b"<!"))
	}

	fn raw_string_end(&self, after_prefix: usize) -> Option<usize> {
		let hashes = self.bytes[after_prefix..]
			.iter()
			.take_while(|&&b| b == b'#')
			.count();
		let body = after_prefix + hashes;
		if self.bytes.get(body) != Some(&b'"') {
			return None;
		}
		let mut at = body + 1;
		while let Some(offset) = self.bytes[at..].iter().position(|&b| b == b'"') {
			let quote = at + offset;
			let closing = self.bytes[quote + 1..]
				.iter()
				.take(hashes)
				.take_while(|&&b| b == b'#')
				.count();
			if closing == hashes {
				return Some(quote + 1 + hashes);
			}
			at = quote + 1;
		}
		Some(self.bytes.len())
	}
}
