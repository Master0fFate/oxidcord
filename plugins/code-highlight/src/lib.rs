mod palette;
mod scan;
mod syntax;

use plugin_sdk::model::{CodeBlockNode, CodeSpan, Color, MessageFull, MessageMeta};
use plugin_sdk::patch::PatchExt;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Md, Plugin, PluginExt, export_plugin, storage};

use crate::palette::Palette;
use crate::scan::Kind;

const THEME_KEY: &str = "theme";
const MAX_CODE_BYTES: usize = 16 * 1024;
const MAX_SPAN_BYTES: usize = 480;

struct CodeHighlight {
	palette: &'static Palette,
}

impl Default for CodeHighlight {
	fn default() -> Self {
		Self {
			palette: Palette::find(""),
		}
	}
}

struct Block<'a> {
	index: u32,
	language: &'a str,
	code: &'a str,
}

fn collect<'a>(nodes: &'a [Md], next: &mut u32, found: &mut Vec<Block<'a>>) {
	for node in nodes {
		match node {
			Md::CodeBlock { lang, code } => {
				if let Some(language) = lang {
					found.push(Block {
						index: *next,
						language,
						code,
					});
				}
				*next += 1;
			}
			Md::Bold(children)
			| Md::Italic(children)
			| Md::Underline(children)
			| Md::Strike(children)
			| Md::Spoiler(children)
			| Md::Quote(children)
			| Md::Subtext(children)
			| Md::Heading { children, .. }
			| Md::Styled { children, .. } => collect(children, next, found),
			Md::Link {
				text: Some(children), ..
			} => collect(children, next, found),
			Md::List { items, .. } => {
				for item in items {
					collect(item, next, found);
				}
			}
			_ => {}
		}
	}
}

impl CodeHighlight {
	fn push_spans(&self, line: &mut Vec<CodeSpan>, kind: Kind, text: &str) {
		let color = self.palette.color(kind).map(|rgb| Color::Rgba(rgb << 8 | 0xff));
		let mut rest = text;
		while !rest.is_empty() {
			let mut cut = rest.len().min(MAX_SPAN_BYTES);
			while !rest.is_char_boundary(cut) {
				cut -= 1;
			}
			let (head, tail) = rest.split_at(cut);
			line.push(CodeSpan {
				text: head.to_owned(),
				color: color.clone(),
				bold: false,
				italic: kind == Kind::Comment,
			});
			rest = tail;
		}
	}

	fn highlight(&self, language: &str, code: &str) -> Option<CodeBlockNode> {
		let syntax = syntax::find(language)?;
		if code.len() > MAX_CODE_BYTES {
			return None;
		}
		let mut lines: Vec<Vec<CodeSpan>> = Vec::new();
		let mut line: Vec<CodeSpan> = Vec::new();
		for token in scan::scan(code, syntax) {
			let mut segments = code[token.start..token.end].split('\n').peekable();
			while let Some(segment) = segments.next() {
				self.push_spans(
					&mut line,
					token.kind,
					segment.strip_suffix('\r').unwrap_or(segment),
				);
				if segments.peek().is_some() {
					lines.push(std::mem::take(&mut line));
				}
			}
		}
		if !line.is_empty() || lines.is_empty() {
			lines.push(line);
		}
		Some(CodeBlockNode {
			language: language.split_whitespace().next().map(str::to_owned),
			lines,
		})
	}
}

impl Plugin for CodeHighlight {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{THEME_KEY}")) {
			self.palette = Palette::find(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == THEME_KEY
		{
			self.palette = Palette::find(&value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"theme":{"type":"string","title":"Theme","description":"Color palette of the code","enum":["dark-plus","one-dark","monokai","github-light"],"default":"dark-plus"},"bgOpacity":{"type":"integer","title":"Background opacity","description":"Not applicable, the client draws the code background","minimum":0,"maximum":100,"default":100}}}"#
			.to_owned()
	}
}

impl PluginExt for CodeHighlight {
	fn intercept_message_ext(
		&mut self,
		_msg: &MessageFull,
		_meta: &MessageMeta,
		content: Vec<Md>,
	) -> PatchExt {
		let mut blocks = Vec::new();
		collect(&content, &mut 0, &mut blocks);
		blocks
			.iter()
			.filter_map(|block| {
				self.highlight(block.language, block.code)
					.map(|node| (block.index, node))
			})
			.fold(PatchExt::keep(), |patch, (index, node)| {
				patch.code_block(index, node)
			})
	}
}

export_plugin!(CodeHighlight, ext);
