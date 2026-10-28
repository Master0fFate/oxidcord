use plugin_sdk::{Md, MessageView, OutgoingMessage, Plugin, SendDecision, export_plugin};

const FENCE: &str = "```";
const TAB_WIDTH: &str = "    ";

#[derive(Default)]
struct Unindent;

impl Plugin for Unindent {
	fn render_message(&mut self, _msg: &MessageView, ast: Vec<Md>) -> Option<Vec<Md>> {
		let mut changed = false;
		let nodes = unindent_blocks(ast, &mut changed);
		changed.then_some(nodes)
	}

	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		let content = unindent_fences(&msg.content);
		if content == msg.content {
			SendDecision::Pass
		} else {
			SendDecision::Replace(OutgoingMessage { content, ..msg })
		}
	}
}

fn unindent_blocks(nodes: Vec<Md>, changed: &mut bool) -> Vec<Md> {
	nodes
		.into_iter()
		.map(|node| match node {
			Md::CodeBlock { lang, code } => {
				let trimmed = unindent(&code);
				*changed |= trimmed != code;
				Md::CodeBlock { lang, code: trimmed }
			}
			Md::Quote(children) => Md::Quote(unindent_blocks(children, changed)),
			Md::List {
				ordered,
				start,
				items,
			} => Md::List {
				ordered,
				start,
				items: items
					.into_iter()
					.map(|item| unindent_blocks(item, changed))
					.collect(),
			},
			other => other,
		})
		.collect()
}

fn unindent_fences(content: &str) -> String {
	let mut out = String::with_capacity(content.len());
	let mut rest = content;
	while let Some(open) = rest.find(FENCE) {
		let inner_start = open + FENCE.len();
		let Some(length) = rest[inner_start..].find(FENCE) else {
			break;
		};
		out.push_str(&rest[..inner_start]);
		out.push_str(&unindent_fence(&rest[inner_start..inner_start + length]));
		out.push_str(FENCE);
		rest = &rest[inner_start + length + FENCE.len()..];
	}
	out.push_str(rest);
	out
}

fn unindent_fence(inner: &str) -> String {
	let Some((head, body)) = inner.split_once('\n') else {
		return inner.to_owned();
	};
	match body.strip_suffix('\n') {
		Some(body) => format!("{head}\n{}\n", unindent(body)),
		None => format!("{head}\n{}", unindent(body)),
	}
}

fn leading_spaces(line: &str) -> usize {
	line.len() - line.trim_start_matches(' ').len()
}

fn unindent(text: &str) -> String {
	let spaced = text.replace('\t', TAB_WIDTH);
	let indent = spaced
		.split('\n')
		.filter(|line| !line.trim().is_empty())
		.map(leading_spaces)
		.min()
		.unwrap_or(0);
	if indent == 0 {
		return spaced;
	}
	spaced
		.split('\n')
		.map(|line| {
			if leading_spaces(line) >= indent {
				&line[indent..]
			} else {
				line
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
}

export_plugin!(Unindent);
