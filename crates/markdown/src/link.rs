use crate::ast::Node;

const SCAN_LIMIT: usize = 4096;

pub(crate) struct Masked<'a> {
	pub text: &'a str,
	pub url: &'a str,
	pub suppress_embed: bool,
	pub end: usize,
}

fn scheme_len(text: &str) -> Option<usize> {
	if text.starts_with("https://") {
		Some(8)
	} else if text.starts_with("http://") {
		Some(7)
	} else {
		None
	}
}

pub(crate) fn bare(src: &str, at: usize) -> Option<usize> {
	let bytes = src.as_bytes();
	if at > 0 && bytes[at - 1].is_ascii_alphanumeric() {
		return None;
	}
	let start = at + scheme_len(&src[at..])?;
	let mut end = src[start..]
		.find(|c: char| c.is_whitespace() || c == '<')
		.map_or(src.len(), |offset| start + offset);
	let count = |needle: u8| bytes[start..end].iter().filter(|&&b| b == needle).count();
	let (round_open, mut round_close) = (count(b'('), count(b')'));
	let (square_open, mut square_close) = (count(b'['), count(b']'));
	while end > start {
		match bytes[end - 1] {
			b'.' | b',' | b':' | b';' | b'!' | b'?' | b'"' | b'\'' => {}
			b')' if round_close > round_open => round_close -= 1,
			b']' if square_close > square_open => square_close -= 1,
			_ => break,
		}
		end -= 1;
	}
	(end > start).then_some(end)
}

pub(crate) fn angle(src: &str, open: usize) -> Option<(Node, usize)> {
	let bytes = src.as_bytes();
	let start = open + 1;
	let body = start + scheme_len(&src[start..])?;
	let limit = bytes.len().min(start + SCAN_LIMIT);
	let mut end = body;
	while end < limit {
		match bytes[end] {
			b'>' => {
				if end == body {
					return None;
				}
				let link = Node::Link {
					url: src[start..end].to_owned(),
					text: None,
					suppress_embed: true,
				};
				return Some((link, end + 1));
			}
			b'<' => return None,
			b if b.is_ascii_whitespace() => return None,
			_ => end += 1,
		}
	}
	None
}

pub(crate) fn masked(src: &str, open: usize) -> Option<Masked<'_>> {
	let bytes = src.as_bytes();
	let limit = bytes.len().min(open + SCAN_LIMIT);
	let close = matching_bracket(bytes, open, limit)?;
	if close == open + 1 || bytes.get(close + 1) != Some(&b'(') {
		return None;
	}
	let at = skip_whitespace(bytes, close + 2, limit);
	let (url_start, url_end, suppress_embed) = if bytes.get(at) == Some(&b'<') {
		let start = at + 1;
		let offset = bytes[start..limit]
			.iter()
			.position(|&b| b == b'>' || b == b'<' || b.is_ascii_whitespace())?;
		if bytes[start + offset] != b'>' {
			return None;
		}
		(start, start + offset, true)
	} else {
		(at, plain_url_end(bytes, at, limit)?, false)
	};
	let after = skip_whitespace(bytes, url_end + usize::from(suppress_embed), limit);
	if bytes.get(after) != Some(&b')') {
		return None;
	}
	let url = &src[url_start..url_end];
	if !scheme_len(url).is_some_and(|scheme| url.len() > scheme) {
		return None;
	}
	Some(Masked {
		text: &src[open + 1..close],
		url,
		suppress_embed,
		end: after + 1,
	})
}

fn plain_url_end(bytes: &[u8], start: usize, limit: usize) -> Option<usize> {
	let mut depth = 0usize;
	for (offset, &b) in bytes[start..limit].iter().enumerate() {
		match b {
			b'(' => depth += 1,
			b')' if depth == 0 => return Some(start + offset),
			b')' => depth -= 1,
			b if b.is_ascii_whitespace() => return Some(start + offset),
			_ => {}
		}
	}
	None
}

fn matching_bracket(bytes: &[u8], open: usize, limit: usize) -> Option<usize> {
	let mut depth = 0usize;
	let mut at = open;
	while at < limit {
		match bytes[at] {
			b'\\' => at += 1,
			b'[' => depth += 1,
			b']' => {
				depth -= 1;
				if depth == 0 {
					return Some(at);
				}
			}
			_ => {}
		}
		at += 1;
	}
	None
}

fn skip_whitespace(bytes: &[u8], mut at: usize, limit: usize) -> usize {
	while at < limit && bytes[at].is_ascii_whitespace() {
		at += 1;
	}
	at
}
