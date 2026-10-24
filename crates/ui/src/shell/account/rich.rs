use std::ops::Range;

use gpui::{
	AnyElement, FontStyle, FontWeight, HighlightStyle, IntoElement, ParentElement as _, StrikethroughStyle,
	Styled as _, StyledText, UnderlineStyle, div,
};
use markdown::Node;

use crate::theme::{Theme, with_alpha};

#[derive(Default)]
struct Paragraph {
	text: String,
	runs: Vec<(Range<usize>, HighlightStyle)>,
	codes: Vec<Range<usize>>,
}

impl Paragraph {
	fn push(&mut self, text: &str, style: &HighlightStyle, code: bool) {
		if text.is_empty() {
			return;
		}
		let start = self.text.len();
		self.text.push_str(text);
		let range = start..self.text.len();
		if *style != HighlightStyle::default() {
			self.runs.push((range.clone(), *style));
		}
		if code {
			self.codes.push(range);
		}
	}
}

struct Builder<'a> {
	theme: &'a Theme,
	blocks: Vec<AnyElement>,
	current: Paragraph,
}

fn merge(base: &HighlightStyle, apply: impl FnOnce(&mut HighlightStyle)) -> HighlightStyle {
	let mut next = *base;
	apply(&mut next);
	next
}

impl<'a> Builder<'a> {
	fn flush(&mut self) {
		let paragraph = std::mem::take(&mut self.current);
		if paragraph.text.is_empty() {
			return;
		}
		let code_font = self.theme.code_font.clone();
		let styled = StyledText::new(paragraph.text)
			.with_highlights(paragraph.runs)
			.with_font_family_overrides(
				paragraph
					.codes
					.into_iter()
					.map(|range| (range, code_font.clone())),
			);
		self.blocks.push(styled.into_any_element());
	}

	fn inline(&mut self, nodes: &[Node], style: &HighlightStyle) {
		for node in nodes {
			match node {
				Node::Text(text) => self.current.push(text, style, false),
				Node::Bold(children) => {
					let next = merge(style, |next| next.font_weight = Some(FontWeight::BOLD));
					self.inline(children, &next);
				}
				Node::Italic(children) => {
					let next = merge(style, |next| next.font_style = Some(FontStyle::Italic));
					self.inline(children, &next);
				}
				Node::Underline(children) => {
					let next = merge(style, |next| {
						next.underline = Some(UnderlineStyle {
							thickness: gpui::px(1.),
							..UnderlineStyle::default()
						})
					});
					self.inline(children, &next);
				}
				Node::Strike(children) => {
					let next = merge(style, |next| {
						next.strikethrough = Some(StrikethroughStyle {
							thickness: gpui::px(1.),
							..StrikethroughStyle::default()
						})
					});
					self.inline(children, &next);
				}
				Node::Spoiler(children) | Node::Styled { children, .. } => self.inline(children, style),
				Node::InlineImage { alt, .. } => self.current.push(alt, style, false),
				Node::Code(code) => {
					let next = merge(style, |next| {
						next.background_color = Some(with_alpha(self.theme.background_tertiary, 0.6))
					});
					self.current.push(code, &next, true);
				}
				Node::Link { url, text, .. } => {
					let next = merge(style, |next| next.color = Some(self.theme.link));
					match text {
						Some(children) => self.inline(children, &next),
						None => self.current.push(url, &next, false),
					}
				}
				Node::LineBreak => self.current.push("\n", style, false),
				Node::CodeBlock { code, .. } => {
					self.flush();
					self.current.push(code.trim_end(), style, true);
					self.flush();
				}
				Node::Quote(children) | Node::Subtext(children) => {
					self.flush();
					self.inline(children, style);
					self.flush();
				}
				Node::Heading { children, .. } => {
					self.flush();
					let next = merge(style, |next| next.font_weight = Some(FontWeight::BOLD));
					self.inline(children, &next);
					self.flush();
				}
				Node::List { items, .. } => {
					self.flush();
					for item in items {
						self.current.push("\u{2022} ", style, false);
						self.inline(item, style);
						self.flush();
					}
				}
				Node::CustomEmoji { name, .. } => self.current.push(&format!(":{name}:"), style, false),
				Node::UserMention(_) | Node::RoleMention(_) | Node::ChannelMention(_) => {}
				Node::Everyone => self.current.push("@everyone", style, false),
				Node::Here => self.current.push("@here", style, false),
				_ => {}
			}
		}
	}
}

pub fn markdown_text(source: &str, theme: &Theme) -> AnyElement {
	let document = markdown::parse(source);
	let mut builder = Builder {
		theme,
		blocks: Vec::new(),
		current: Paragraph::default(),
	};
	builder.inline(&document.nodes, &HighlightStyle::default());
	builder.flush();
	div()
		.flex()
		.flex_col()
		.gap(theme.space_4)
		.min_w_0()
		.text_size(theme.font_sm)
		.text_color(theme.text_normal)
		.children(builder.blocks)
		.into_any_element()
}
