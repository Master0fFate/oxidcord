use std::fmt::Display;

pub enum Json {
	Null,
	Bool(bool),
	Num(String),
	Str(String),
	List(Vec<Json>),
	Object(Vec<(&'static str, Json)>),
}

const INDENT: &str = "    ";

impl Json {
	pub fn num(value: impl Display) -> Self {
		Self::Num(value.to_string())
	}

	pub fn text(value: &str) -> Self {
		Self::Str(value.to_owned())
	}

	pub fn id(value: u64) -> Self {
		Self::Str(value.to_string())
	}

	pub fn option<T>(value: Option<T>, build: impl FnOnce(T) -> Self) -> Self {
		value.map_or(Self::Null, build)
	}

	pub fn list<T>(items: impl IntoIterator<Item = T>, build: impl FnMut(T) -> Self) -> Self {
		Self::List(items.into_iter().map(build).collect())
	}

	pub fn render(&self, sort_keys: bool) -> String {
		let mut out = String::new();
		self.write(&mut out, 0, sort_keys);
		out
	}

	fn write(&self, out: &mut String, depth: usize, sort_keys: bool) {
		match self {
			Self::Null => out.push_str("null"),
			Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
			Self::Num(value) => out.push_str(value),
			Self::Str(value) => write_string(out, value),
			Self::List(items) => {
				let entries: Vec<(Option<&str>, &Json)> = items.iter().map(|item| (None, item)).collect();
				write_entries(out, depth, ('[', ']'), &entries, sort_keys);
			}
			Self::Object(fields) => {
				let mut entries: Vec<(Option<&str>, &Json)> =
					fields.iter().map(|(key, value)| (Some(*key), value)).collect();
				if sort_keys {
					entries.sort_by_key(|(key, _)| *key);
				}
				write_entries(out, depth, ('{', '}'), &entries, sort_keys);
			}
		}
	}
}

fn write_entries(
	out: &mut String,
	depth: usize,
	(open, close): (char, char),
	entries: &[(Option<&str>, &Json)],
	sort_keys: bool,
) {
	out.push(open);
	if entries.is_empty() {
		out.push(close);
		return;
	}
	out.push('\n');
	for (index, (key, value)) in entries.iter().enumerate() {
		out.push_str(&INDENT.repeat(depth + 1));
		if let Some(key) = key {
			write_string(out, key);
			out.push_str(": ");
		}
		value.write(out, depth + 1, sort_keys);
		if index + 1 < entries.len() {
			out.push(',');
		}
		out.push('\n');
	}
	out.push_str(&INDENT.repeat(depth));
	out.push(close);
}

fn write_string(out: &mut String, value: &str) {
	out.push('"');
	for ch in value.chars() {
		match ch {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			ch if ch < ' ' => out.push_str(&format!("\\u{:04x}", ch as u32)),
			ch => out.push(ch),
		}
	}
	out.push('"');
}
