use crate::ast::{GuildNavKind, Node, TimestampStyle};
use crate::link;

const NAVIGATION: [(&str, GuildNavKind); 4] = [
	("customize>", GuildNavKind::Customize),
	("browse>", GuildNavKind::Browse),
	("guide>", GuildNavKind::Guide),
	("linked-roles>", GuildNavKind::LinkedRoles),
];

const NAME_LIMIT: usize = 32;
const COMMAND_NAME_LIMIT: usize = 128;

pub(crate) fn parse(src: &str, open: usize, allow_links: bool) -> Option<(Node, usize)> {
	let bytes = src.as_bytes();
	let body = open + 1;
	match *bytes.get(body)? {
		b'@' => mention(bytes, body + 1),
		b'#' => {
			let (id, end) = id_then_close(bytes, body + 1)?;
			Some((Node::ChannelMention(id), end))
		}
		b':' => emoji(src, body + 1, false),
		b'a' if bytes.get(body + 1) == Some(&b':') => emoji(src, body + 2, true),
		b't' if bytes.get(body + 1) == Some(&b':') => timestamp(bytes, body + 2),
		b'/' => command(src, body + 1),
		b'i' => navigation(src, body),
		b'h' if allow_links => link::angle(src, open),
		_ => None,
	}
}

fn digits(bytes: &[u8], start: usize) -> Option<(u64, usize)> {
	let mut value = 0u64;
	let mut end = start;
	while let Some(&digit) = bytes.get(end)
		&& digit.is_ascii_digit()
	{
		value = value.checked_mul(10)?.checked_add(u64::from(digit - b'0'))?;
		end += 1;
	}
	(end > start).then_some((value, end))
}

fn expect(bytes: &[u8], at: usize, byte: u8) -> Option<usize> {
	(bytes.get(at) == Some(&byte)).then_some(at + 1)
}

fn id_then_close(bytes: &[u8], start: usize) -> Option<(u64, usize)> {
	let (id, end) = digits(bytes, start)?;
	Some((id, expect(bytes, end, b'>')?))
}

fn mention(bytes: &[u8], start: usize) -> Option<(Node, usize)> {
	let (role, digits_start) = match bytes.get(start)? {
		b'!' => (false, start + 1),
		b'&' => (true, start + 1),
		_ => (false, start),
	};
	let (id, end) = id_then_close(bytes, digits_start)?;
	let node = if role {
		Node::RoleMention(id)
	} else {
		Node::UserMention(id)
	};
	Some((node, end))
}

fn emoji(src: &str, start: usize, animated: bool) -> Option<(Node, usize)> {
	let bytes = src.as_bytes();
	let length = bytes[start..]
		.iter()
		.take(NAME_LIMIT + 1)
		.take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
		.count();
	if length == 0 || length > NAME_LIMIT {
		return None;
	}
	let after_name = expect(bytes, start + length, b':')?;
	let (id, end) = id_then_close(bytes, after_name)?;
	let name = src[start..start + length].to_owned();
	Some((Node::CustomEmoji { name, id, animated }, end))
}

fn timestamp(bytes: &[u8], start: usize) -> Option<(Node, usize)> {
	let negative = bytes.get(start) == Some(&b'-');
	let (magnitude, mut end) = digits(bytes, start + usize::from(negative))?;
	let magnitude = i64::try_from(magnitude).ok()?;
	let unix = if negative { -magnitude } else { magnitude };
	let mut style = TimestampStyle::default();
	if bytes.get(end) == Some(&b':') {
		style = TimestampStyle::from_code(*bytes.get(end + 1)?)?;
		end += 2;
	}
	Some((Node::Timestamp { unix, style }, expect(bytes, end, b'>')?))
}

fn command(src: &str, start: usize) -> Option<(Node, usize)> {
	let bytes = src.as_bytes();
	let length = bytes[start..]
		.iter()
		.take(COMMAND_NAME_LIMIT)
		.position(|&b| b == b':')?;
	let name = &src[start..start + length];
	if !is_command_name(name) {
		return None;
	}
	let (id, end) = id_then_close(bytes, start + length + 1)?;
	Some((
		Node::CommandMention {
			name: name.to_owned(),
			id,
		},
		end,
	))
}

fn is_command_name(name: &str) -> bool {
	let mut parts = 0;
	for part in name.split(' ') {
		parts += 1;
		let valid = !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
		if !valid || parts > 3 {
			return false;
		}
	}
	true
}

fn navigation(src: &str, body: usize) -> Option<(Node, usize)> {
	let tail = src[body..].strip_prefix("id:")?;
	NAVIGATION.iter().find_map(|&(suffix, kind)| {
		tail.starts_with(suffix)
			.then(|| (Node::GuildNav(kind), body + 3 + suffix.len()))
	})
}
