use crate::syntax::Syntax;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
	Plain,
	Keyword,
	Type,
	Function,
	String,
	Number,
	Comment,
	Punctuation,
	Property,
	Variable,
}

pub struct Token {
	pub kind: Kind,
	pub start: usize,
	pub end: usize,
}

struct Scanner<'a> {
	code: &'a str,
	bytes: &'a [u8],
	syntax: &'a Syntax,
	tokens: Vec<Token>,
	in_tag: bool,
	tag_name_next: bool,
}

pub fn scan(code: &str, syntax: &Syntax) -> Vec<Token> {
	let mut scanner = Scanner {
		code,
		bytes: code.as_bytes(),
		syntax,
		tokens: Vec::new(),
		in_tag: false,
		tag_name_next: false,
	};
	let mut at = 0;
	while at < scanner.bytes.len() {
		let (kind, end) = if syntax.markup {
			scanner.markup_token(at)
		} else {
			scanner.token(at)
		};
		scanner.emit(kind, at, end);
		at = end;
	}
	scanner.tokens
}

impl Scanner<'_> {
	fn emit(&mut self, kind: Kind, start: usize, end: usize) {
		match self.tokens.last_mut() {
			Some(last) if last.kind == kind && last.end == start => last.end = end,
			_ => self.tokens.push(Token { kind, start, end }),
		}
	}

	fn byte(&self, at: usize) -> Option<u8> {
		self.bytes.get(at).copied()
	}

	fn run(&self, from: usize, accept: impl Fn(u8) -> bool) -> usize {
		self.bytes[from..]
			.iter()
			.position(|byte| !accept(*byte))
			.map_or(self.bytes.len(), |offset| from + offset)
	}

	fn find(&self, from: usize, needle: &str) -> usize {
		self.code[from..]
			.find(needle)
			.map_or(self.bytes.len(), |offset| from + offset + needle.len())
	}

	fn line_end(&self, from: usize) -> usize {
		self.bytes[from..]
			.iter()
			.position(|byte| *byte == b'\n')
			.map_or(self.bytes.len(), |offset| from + offset)
	}

	fn is_identifier_start(&self, byte: u8) -> bool {
		byte.is_ascii_alphabetic()
			|| byte == b'_'
			|| byte >= 0x80
			|| (byte == b'$' && self.syntax.dollar_identifiers)
	}

	fn is_identifier_part(&self, byte: u8) -> bool {
		self.is_identifier_start(byte)
			|| byte.is_ascii_digit()
			|| (byte == b'-' && self.syntax.dash_identifiers)
	}

	fn at_line_start(&self, at: usize) -> bool {
		self.bytes[..at]
			.iter()
			.rev()
			.find(|byte| **byte != b' ' && **byte != b'\t')
			.is_none_or(|byte| *byte == b'\n')
	}

	fn next_non_blank(&self, from: usize) -> Option<(usize, u8)> {
		let at = self.run(from, |byte| byte == b' ' || byte == b'\t');
		self.byte(at).map(|byte| (at, byte))
	}

	fn followed_by_key(&self, end: usize) -> bool {
		let separator = self.syntax.key_separator;
		match self.next_non_blank(end) {
			Some((at, byte)) if separator != 0 && byte == separator => {
				!matches!(self.byte(at + 1), Some(next) if next == separator || next == b'/')
			}
			_ => false,
		}
	}

	fn comment_end(&self, at: usize) -> Option<usize> {
		let rest = &self.code[at..];
		if self
			.syntax
			.line_comments
			.iter()
			.any(|prefix| rest.starts_with(prefix))
		{
			return Some(self.line_end(at));
		}
		if self.syntax.block_comment && rest.starts_with("/*") {
			return Some(self.find(at + 2, "*/"));
		}
		None
	}

	fn token(&self, at: usize) -> (Kind, usize) {
		let byte = self.bytes[at];
		if byte.is_ascii_whitespace() {
			return (Kind::Plain, self.run(at, |next| next.is_ascii_whitespace()));
		}
		if let Some(end) = self.comment_end(at) {
			return (Kind::Comment, end);
		}
		if self.syntax.quotes.contains(&byte) {
			return self.string(at, byte);
		}
		if byte.is_ascii_digit()
			|| (byte == b'.' && self.byte(at + 1).is_some_and(|next| next.is_ascii_digit()))
		{
			return (Kind::Number, self.number(at));
		}
		if self.is_identifier_start(byte) {
			return self.word(at);
		}
		match byte {
			b'$' if self.syntax.variables => (Kind::Variable, self.variable(at)),
			b'@' if self.syntax.decorators || self.syntax.at_rules => self.at_word(at),
			b'#' if self.syntax.hash_colors => self.hash_color(at),
			b'#' if self.syntax.preprocessor && self.at_line_start(at) => {
				let name = self.run(at + 1, |next| next == b' ' || next == b'\t');
				(Kind::Keyword, self.run(name, |next| next.is_ascii_alphabetic()))
			}
			b'[' if self.syntax.sections && self.at_line_start(at) => self.section(at),
			_ => (Kind::Punctuation, at + 1),
		}
	}

	fn string(&self, at: usize, quote: u8) -> (Kind, usize) {
		let triple = [quote; 3];
		if self.syntax.triple_quotes && self.bytes[at..].starts_with(&triple) {
			let closing = std::str::from_utf8(&triple).unwrap_or("\"\"\"");
			return (Kind::String, self.find(at + 3, closing));
		}
		if self.syntax.lifetimes
			&& quote == b'\''
			&& self
				.byte(at + 1)
				.is_some_and(|next| next.is_ascii_alphabetic() || next == b'_')
		{
			let end = self.run(at + 1, |next| next.is_ascii_alphanumeric() || next == b'_');
			if self.byte(end) != Some(b'\'') {
				return (Kind::Type, end);
			}
		}
		let multiline = self.syntax.multiline_strings || quote == b'`';
		let mut end = at + 1;
		while let Some(byte) = self.byte(end) {
			end += 1;
			if byte == b'\\' {
				end += 1;
			} else if byte == quote {
				break;
			} else if byte == b'\n' && !multiline {
				end -= 1;
				break;
			}
		}
		let end = end.min(self.bytes.len());
		let kind = if self.followed_by_key(end) {
			Kind::Property
		} else {
			Kind::String
		};
		(kind, end)
	}

	fn number(&self, at: usize) -> usize {
		let mut end = at;
		while let Some(byte) = self.byte(end) {
			let exponent_sign = (byte == b'+' || byte == b'-')
				&& end > at && matches!(self.bytes[end - 1], b'e' | b'E')
				&& self.byte(end + 1).is_some_and(|next| next.is_ascii_digit());
			let fraction = byte == b'.' && self.byte(end + 1).is_some_and(|next| next.is_ascii_digit());
			if byte.is_ascii_alphanumeric() || byte == b'_' || fraction || exponent_sign {
				end += 1;
			} else {
				break;
			}
		}
		end
	}

	fn word(&self, at: usize) -> (Kind, usize) {
		let end = self.run(at, |byte| self.is_identifier_part(byte));
		let word = &self.code[at..end];
		let listed = |words: &[&str]| {
			if self.syntax.case_insensitive {
				words.iter().any(|candidate| candidate.eq_ignore_ascii_case(word))
			} else {
				words.contains(&word)
			}
		};
		let kind = if listed(self.syntax.keywords) || listed(self.syntax.more_keywords) {
			Kind::Keyword
		} else if self.followed_by_key(end) {
			Kind::Property
		} else if self.syntax.macros && self.byte(end) == Some(b'!') && self.byte(end + 1) != Some(b'=') {
			return (Kind::Function, end + 1);
		} else if self.byte(end) == Some(b'(') {
			Kind::Function
		} else if listed(self.syntax.types)
			|| (self.syntax.capitalized_types && word.as_bytes()[0].is_ascii_uppercase())
		{
			Kind::Type
		} else {
			Kind::Plain
		};
		(kind, end)
	}

	fn variable(&self, at: usize) -> usize {
		match self.byte(at + 1) {
			Some(b'{') => self.bytes[at..]
				.iter()
				.position(|byte| *byte == b'}' || *byte == b'\n')
				.map_or(self.bytes.len(), |offset| at + offset + 1),
			Some(byte) if byte.is_ascii_alphanumeric() || byte == b'_' => {
				self.run(at + 1, |next| next.is_ascii_alphanumeric() || next == b'_')
			}
			Some(b'@' | b'#' | b'?' | b'!' | b'$' | b'*' | b'-') => at + 2,
			_ => at + 1,
		}
	}

	fn at_word(&self, at: usize) -> (Kind, usize) {
		let end = self.run(at + 1, |byte| self.is_identifier_part(byte) || byte == b'.');
		if end == at + 1 {
			return (Kind::Punctuation, end);
		}
		let kind = if self.syntax.at_rules {
			Kind::Keyword
		} else {
			Kind::Function
		};
		(kind, end)
	}

	fn hash_color(&self, at: usize) -> (Kind, usize) {
		let end = self.run(at + 1, |byte| byte.is_ascii_hexdigit());
		if (4..=9).contains(&(end - at)) {
			(Kind::Number, end)
		} else {
			(Kind::Punctuation, at + 1)
		}
	}

	fn section(&self, at: usize) -> (Kind, usize) {
		let opening = self.run(at, |byte| byte == b'[');
		match self.bytes[opening..]
			.iter()
			.position(|byte| *byte == b']' || *byte == b'\n')
		{
			Some(offset) if self.bytes[opening + offset] == b']' => {
				(Kind::Type, self.run(opening + offset, |byte| byte == b']'))
			}
			_ => (Kind::Punctuation, opening),
		}
	}

	fn markup_token(&mut self, at: usize) -> (Kind, usize) {
		let byte = self.bytes[at];
		if !self.in_tag {
			return self.markup_text(at, byte);
		}
		if byte.is_ascii_whitespace() {
			return (Kind::Plain, self.run(at, |next| next.is_ascii_whitespace()));
		}
		match byte {
			b'>' => {
				self.in_tag = false;
				(Kind::Punctuation, at + 1)
			}
			b'"' | b'\'' => self.string(at, byte),
			_ if self.is_identifier_start(byte) => {
				let end = self.run(at, |next| self.is_identifier_part(next) || next == b':');
				let kind = if self.tag_name_next {
					Kind::Keyword
				} else {
					Kind::Property
				};
				self.tag_name_next = false;
				(kind, end)
			}
			_ => (Kind::Punctuation, at + 1),
		}
	}

	fn markup_text(&mut self, at: usize, byte: u8) -> (Kind, usize) {
		let rest = &self.code[at..];
		if rest.starts_with("<!--") {
			return (Kind::Comment, self.find(at + 4, "-->"));
		}
		let opens_tag = byte == b'<'
			&& self
				.byte(at + 1)
				.is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, b'/' | b'!' | b'?'));
		if opens_tag {
			self.in_tag = true;
			self.tag_name_next = true;
			let marker = usize::from(matches!(self.byte(at + 1), Some(b'/' | b'!' | b'?')));
			return (Kind::Punctuation, at + 1 + marker);
		}
		if byte == b'&' {
			let end = self.run(at + 1, |next| next.is_ascii_alphanumeric() || next == b'#');
			if self.byte(end) == Some(b';') {
				return (Kind::Number, end + 1);
			}
		}
		let end = self.run(at + 1, |next| next != b'<' && next != b'&');
		(Kind::Plain, end)
	}
}
