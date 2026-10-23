pub(crate) enum Fence<'a> {
	Block {
		lang: Option<&'a str>,
		code: &'a str,
		end: usize,
	},
	Empty,
	Unclosed,
}

pub(crate) fn parse(src: &str, open: usize) -> Fence<'_> {
	let body_start = open + 3;
	let Some(offset) = src[body_start..].find("```") else {
		return Fence::Unclosed;
	};
	let close = body_start + offset;
	let body = &src[body_start..close];
	let (lang, code) = match split_language(body) {
		Some((lang, code)) if !code.is_empty() => (Some(lang), code),
		_ => (None, trim_newlines(body)),
	};
	if code.is_empty() {
		return Fence::Empty;
	}
	Fence::Block {
		lang,
		code,
		end: close + 3,
	}
}

pub(crate) fn skip_newline(src: &str, end: usize) -> usize {
	let rest = &src.as_bytes()[end..];
	if rest.starts_with(b"\r\n") {
		end + 2
	} else if rest.starts_with(b"\n") {
		end + 1
	} else {
		end
	}
}

fn split_language(body: &str) -> Option<(&str, &str)> {
	let end = body.find(|c: char| !is_language_char(c))?;
	if end == 0 {
		return None;
	}
	let rest = &body[end..];
	let after = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;
	Some((&body[..end], trim_newlines(after)))
}

fn is_language_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '.' | '#')
}

fn trim_newlines(text: &str) -> &str {
	text.trim_matches(['\r', '\n'])
}
