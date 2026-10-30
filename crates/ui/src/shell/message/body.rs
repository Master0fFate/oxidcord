use std::cell::Cell;
use std::collections::HashSet;
use std::ops::Range;

use chrono::{DateTime, Local};
use gpui::{
	AnyElement, App, ElementId, FontStyle, FontWeight, HighlightStyle, IntoElement, ParentElement as _,
	SharedString, StrikethroughStyle, Styled as _, StyledText, UnderlineStyle, div, img,
	prelude::FluentBuilder as _, px,
};
use markdown::highlight::{TokenKind, highlight};
use markdown::{Color, Document, GuildNavKind, Node, TimestampStyle};
use media::SizeHint;
use plugin_host::api::model::{CodeBlockNode, EmojiRef};
use protocol::Snowflake;

use super::selectable::SelectableRun;
use super::{Dispatch, Prefs, RowAction};
use crate::format;
use crate::i18n::Translate as _;
use crate::images::Images;
use crate::lookup::Lookup;
use crate::shell::plugins::{MenuTarget, PatchEffects, plugin_menu, resolve_color};
use crate::theme::{Theme, rem, with_alpha};

const EMOJI_SIZE: f32 = 22.;
const JUMBO_EMOJI_SIZE: f32 = 48.;
const JUMBO_TEXT_SIZE: f32 = 40.;
const SNOWFLAKE_TIME_SHIFT: u32 = 22;
const RUN_INDEX_BITS: u32 = 20;
const RUN_INDEX_MASK: u64 = (1 << RUN_INDEX_BITS) - 1;
const SELECTION_ALPHA: f32 = 0.35;

pub struct BodyEnv<'a> {
	pub theme: &'a Theme,
	pub lookup: &'a Lookup<'a>,
	pub dispatch: &'a Dispatch,
	pub message_id: u64,
	pub revealed: &'a HashSet<(u64, usize)>,
	pub jumbo: bool,
	pub trailer: Option<SharedString>,
	pub text_size: Option<gpui::Rems>,
	pub color: Option<gpui::Hsla>,
	pub prefs: Prefs,
	patch: Option<&'a PatchEffects>,
	channel_id: Option<u64>,
	emoji_menus: Cell<u32>,
	spoilers: Cell<usize>,
	paragraphs: Cell<u64>,
	code_blocks: Cell<u32>,
}

impl<'a> BodyEnv<'a> {
	pub fn new(
		theme: &'a Theme,
		lookup: &'a Lookup<'a>,
		dispatch: &'a Dispatch,
		message_id: u64,
		revealed: &'a HashSet<(u64, usize)>,
		prefs: Prefs,
	) -> Self {
		Self {
			theme,
			lookup,
			dispatch,
			message_id,
			revealed,
			jumbo: false,
			trailer: None,
			text_size: None,
			color: None,
			prefs,
			patch: None,
			channel_id: None,
			emoji_menus: Cell::new(0),
			spoilers: Cell::new(0),
			paragraphs: Cell::new(0),
			code_blocks: Cell::new(0),
		}
	}

	pub fn patch(mut self, patch: Option<&'a PatchEffects>) -> Self {
		self.patch = patch;
		self
	}

	pub fn in_channel(mut self, channel_id: u64) -> Self {
		self.channel_id = Some(channel_id);
		self
	}

	pub(super) fn effects(&self) -> Option<&PatchEffects> {
		self.patch
	}

	pub(super) fn channel_id(&self) -> Option<u64> {
		self.channel_id
	}

	pub(super) fn channel(&self) -> Option<Snowflake> {
		self.channel_id.map(Snowflake)
	}

	pub(super) fn message_at(&self) -> Option<(Option<Snowflake>, Snowflake, Snowflake)> {
		Some((self.lookup.guild, self.channel()?, Snowflake(self.message_id)))
	}

	pub(super) fn next_emoji_menu(&self) -> u32 {
		let index = self.emoji_menus.get();
		self.emoji_menus.set(index + 1);
		index
	}

	pub(super) fn is_revealed(&self, index: usize) -> bool {
		self.patch.is_some_and(PatchEffects::reveals_spoilers)
			|| self.revealed.contains(&(self.message_id, index))
	}

	fn next_code_block(&self) -> u32 {
		let index = self.code_blocks.get();
		self.code_blocks.set(index + 1);
		index
	}

	pub fn jumbo(mut self, jumbo: bool) -> Self {
		self.jumbo = jumbo;
		self
	}

	pub fn trailer(mut self, trailer: Option<SharedString>) -> Self {
		self.trailer = trailer;
		self
	}

	pub fn text_size(mut self, size: gpui::Rems) -> Self {
		self.text_size = Some(size);
		self
	}

	pub fn color(mut self, color: Option<gpui::Hsla>) -> Self {
		self.color = color;
		self
	}

	fn scaled(&self, size: gpui::Rems) -> gpui::Rems {
		size * self.prefs.font_scale
	}

	pub(super) fn next_spoiler(&self) -> usize {
		let index = self.spoilers.get();
		self.spoilers.set(index + 1);
		index
	}

	fn next_paragraph(&self) -> (ElementId, u64) {
		let index = self.paragraphs.get();
		self.paragraphs.set(index + 1);
		let id = SharedString::from(format!("md-{}-{index}", self.message_id)).into();
		let order = ((self.message_id >> SNOWFLAKE_TIME_SHIFT) << RUN_INDEX_BITS) | (index & RUN_INDEX_MASK);
		(id, order)
	}

	fn selectable(&self, text: SharedString, styled: StyledText) -> SelectableRun {
		let (id, order) = self.next_paragraph();
		SelectableRun::new(
			id,
			text,
			styled,
			order,
			with_alpha(self.theme.brand, SELECTION_ALPHA),
		)
	}
}

pub fn render_body(document: &Document, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let size = if env.jumbo {
		rem(JUMBO_TEXT_SIZE)
	} else {
		env.scaled(env.text_size.unwrap_or(theme.font_md))
	};
	let line = if env.jumbo {
		rem(JUMBO_EMOJI_SIZE)
	} else {
		size * 1.375
	};
	div()
		.flex()
		.flex_col()
		.min_w_0()
		.text_size(size)
		.line_height(line)
		.text_color(env.color.unwrap_or(theme.text_normal))
		.children(Blocks::new(env, cx).render(&document.nodes, true))
		.into_any_element()
}

#[derive(Clone)]
enum Click {
	Link(String),
	Spoiler(usize),
	User(Snowflake),
}

#[derive(Clone)]
enum Glyph {
	Emoji {
		id: u64,
		animated: bool,
		name: String,
	},
	Image {
		url: SharedString,
		width: f32,
		height: f32,
	},
	Unicode(String),
}

#[derive(Clone)]
struct EmojiAt {
	at: usize,
	glyph: Glyph,
}

#[derive(Default, Clone)]
struct Inline {
	text: String,
	runs: Vec<(Range<usize>, HighlightStyle)>,
	codes: Vec<Range<usize>>,
	clicks: Vec<(Range<usize>, Click)>,
	emoji: Vec<EmojiAt>,
}

#[derive(Clone, Default)]
struct Style {
	highlight: HighlightStyle,
	spoiler: Option<usize>,
	link: Option<String>,
	user: Option<Snowflake>,
}

fn intersect(range: &Range<usize>, window: &Range<usize>) -> Option<Range<usize>> {
	let start = range.start.max(window.start);
	let end = range.end.min(window.end);
	(start < end).then(|| start - window.start..end - window.start)
}

impl Inline {
	fn push(&mut self, text: &str, style: &Style) {
		if text.is_empty() {
			return;
		}
		let start = self.text.len();
		self.text.push_str(text);
		let range = start..self.text.len();
		if style.highlight != HighlightStyle::default() {
			self.runs.push((range.clone(), style.highlight));
		}
		if let Some(index) = style.spoiler {
			self.clicks.push((range, Click::Spoiler(index)));
		} else if let Some(url) = &style.link {
			self.clicks.push((range, Click::Link(url.clone())));
		} else if let Some(user) = style.user {
			self.clicks.push((range, Click::User(user)));
		}
	}

	fn push_text(&mut self, text: &str, style: &Style) {
		if style.spoiler.is_some() {
			self.push(text, style);
			return;
		}
		let mut cursor = 0;
		for (range, url) in media::twemoji::find(text) {
			self.push(&text[cursor..range.start], style);
			self.emoji.push(EmojiAt {
				at: self.text.len(),
				glyph: Glyph::Unicode(url),
			});
			cursor = range.end;
		}
		self.push(&text[cursor..], style);
	}

	fn is_empty(&self) -> bool {
		self.text.is_empty() && self.emoji.is_empty()
	}

	fn slice(&self, window: Range<usize>) -> Inline {
		Inline {
			text: self.text[window.clone()].to_owned(),
			runs: self
				.runs
				.iter()
				.filter_map(|(range, style)| Some((intersect(range, &window)?, *style)))
				.collect(),
			codes: self
				.codes
				.iter()
				.filter_map(|range| intersect(range, &window))
				.collect(),
			clicks: self
				.clicks
				.iter()
				.filter_map(|(range, click)| Some((intersect(range, &window)?, click.clone())))
				.collect(),
			emoji: Vec::new(),
		}
	}

	fn text_element(self, env: &BodyEnv) -> AnyElement {
		let code_font = env.theme.code_font.clone();
		let text = SharedString::from(self.text);
		let styled = StyledText::new(text.clone())
			.with_highlights(self.runs)
			.with_font_family_overrides(self.codes.into_iter().map(|range| (range, code_font.clone())));
		let run = env.selectable(text, styled);
		if self.clicks.is_empty() {
			return run.into_any_element();
		}
		let ranges: Vec<Range<usize>> = self.clicks.iter().map(|(range, _)| range.clone()).collect();
		let targets: Vec<Click> = self.clicks.into_iter().map(|(_, click)| click).collect();
		let dispatch = env.dispatch.clone();
		let message_id = env.message_id;
		run.on_click(ranges, move |index, window, cx| {
			let action = match &targets[index] {
				Click::Link(url) => RowAction::OpenMessageLink {
					url: url.clone(),
					message_id,
				},
				Click::Spoiler(spoiler) => RowAction::ToggleSpoiler(message_id, *spoiler),
				Click::User(user) => RowAction::OpenProfile {
					user: *user,
					at: window.mouse_position(),
				},
			};
			dispatch(action, window, cx);
		})
		.into_any_element()
	}

	fn into_element(self, env: &BodyEnv, cx: &App) -> AnyElement {
		if self.emoji.is_empty() {
			return div().child(self.text_element(env)).into_any_element();
		}
		let size = if env.jumbo {
			JUMBO_EMOJI_SIZE
		} else {
			EMOJI_SIZE * env.prefs.font_scale
		};
		let mut children: Vec<AnyElement> = Vec::new();
		let mut cursor = 0;
		for emoji in &self.emoji {
			self.push_chunks(cursor..emoji.at, env, &mut children);
			children.push(match &emoji.glyph {
				Glyph::Emoji { id, animated, name } => plugin_menu(
					ElementId::NamedInteger(
						"emoji-menu".into(),
						(env.message_id << 16) ^ u64::from(env.next_emoji_menu()),
					),
					emoji_element(*id, *animated, size, cx),
					"emoji-context-menu",
					MenuTarget::emoji(
						env.lookup.guild,
						env.channel(),
						EmojiRef {
							id: Some(*id),
							name: name.clone(),
							animated: *animated,
						},
					),
					cx,
				),
				Glyph::Image { url, width, height } => image_element(url, *width, *height, cx),
				Glyph::Unicode(url) => unicode_element(url, size, cx),
			});
			cursor = emoji.at;
		}
		self.push_chunks(cursor..self.text.len(), env, &mut children);
		div()
			.flex()
			.flex_wrap()
			.items_center()
			.children(children)
			.into_any_element()
	}

	fn push_chunks(&self, range: Range<usize>, env: &BodyEnv, out: &mut Vec<AnyElement>) {
		let text = &self.text[range.clone()];
		let mut start = 0;
		let mut previous_space = false;
		let mut chunks = Vec::new();
		for (index, character) in text.char_indices() {
			if character == '\n' {
				chunks.push((start..index, true));
				start = index + 1;
				previous_space = false;
				continue;
			}
			if previous_space && !character.is_whitespace() {
				chunks.push((start..index, false));
				start = index;
			}
			previous_space = character.is_whitespace();
		}
		chunks.push((start..text.len(), false));
		for (chunk, line_break) in chunks {
			if !chunk.is_empty() {
				let window = range.start + chunk.start..range.start + chunk.end;
				out.push(self.slice(window).text_element(env));
			}
			if line_break {
				out.push(div().w_full().h(px(0.)).into_any_element());
			}
		}
	}
}

fn emoji_element(id: u64, animated: bool, size: f32, cx: &App) -> AnyElement {
	let pixels = (size * 2.) as u32;
	let url = media::cdn::emoji_url(id, animated, pixels);
	let image = cx.global::<Images>().get(&url, SizeHint::square(pixels));
	div()
		.flex_none()
		.size(rem(size))
		.mx(rem(1.))
		.when_some(image, |slot, image| slot.child(img(image).size_full()))
		.into_any_element()
}

fn unicode_element(url: &str, size: f32, cx: &App) -> AnyElement {
	let pixels = (size * 2.) as u32;
	let image = cx.global::<Images>().get(url, SizeHint::square(pixels));
	div()
		.flex_none()
		.size(rem(size))
		.mx(rem(1.))
		.when_some(image, |slot, image| slot.child(img(image).size_full()))
		.into_any_element()
}

fn image_element(url: &SharedString, width: f32, height: f32, cx: &App) -> AnyElement {
	let image = cx
		.global::<Images>()
		.get(url, SizeHint::new((width * 2.) as u32, (height * 2.) as u32));
	div()
		.flex_none()
		.w(rem(width))
		.h(rem(height))
		.mx(rem(1.))
		.when_some(image, |slot, image| slot.child(img(image).size_full()))
		.into_any_element()
}

fn tint(color: &Color, theme: &Theme) -> Option<gpui::Hsla> {
	match color {
		Color::Token(token) => theme.token(token),
		Color::Rgba(rgba) => Some(gpui::rgba(*rgba).into()),
	}
}

struct Blocks<'a> {
	env: &'a BodyEnv<'a>,
	cx: &'a App,
	out: Vec<AnyElement>,
	inline: Inline,
}

impl<'a> Blocks<'a> {
	fn new(env: &'a BodyEnv<'a>, cx: &'a App) -> Self {
		Self {
			env,
			cx,
			out: Vec::new(),
			inline: Inline::default(),
		}
	}

	fn render(mut self, nodes: &[Node], trailing: bool) -> Vec<AnyElement> {
		for node in nodes {
			if node.is_block() {
				self.flush();
				self.block(node);
			} else {
				self.inline_node(node, &Style::default());
			}
		}
		if trailing && let Some(label) = self.env.trailer.clone() {
			let muted = Style {
				highlight: HighlightStyle {
					color: Some(self.env.theme.text_muted),
					..HighlightStyle::default()
				},
				..Style::default()
			};
			self.inline.push(&format!(" {label}"), &muted);
		}
		self.flush();
		self.out
	}

	fn flush(&mut self) {
		if !self.inline.is_empty() {
			let inline = std::mem::take(&mut self.inline);
			self.out.push(inline.into_element(self.env, self.cx));
		}
	}

	fn nested(&self, nodes: &[Node]) -> Vec<AnyElement> {
		Blocks::new(self.env, self.cx).render(nodes, false)
	}

	fn inline_element(&self, nodes: &[Node]) -> AnyElement {
		let mut blocks = Blocks::new(self.env, self.cx);
		for node in nodes {
			blocks.inline_node(node, &Style::default());
		}
		std::mem::take(&mut blocks.inline).into_element(self.env, self.cx)
	}

	fn children(&mut self, nodes: &[Node], style: &Style) {
		for node in nodes {
			self.inline_node(node, style);
		}
	}

	fn styled(style: &Style, edit: impl FnOnce(&mut HighlightStyle)) -> Style {
		let mut next = style.clone();
		edit(&mut next.highlight);
		next
	}

	fn mention(&mut self, label: String, color: Option<gpui::Hsla>, style: &Style) {
		let theme = self.env.theme;
		let accent = color.unwrap_or(theme.mention_text);
		let background = color.map_or(theme.mention_background, |color| with_alpha(color, 0.2));
		let mention = Self::styled(style, |highlight| {
			highlight.color = Some(accent);
			highlight.background_color = Some(background);
			highlight.font_weight = Some(FontWeight::MEDIUM);
		});
		self.inline.push(&label, &mention);
	}

	fn inline_node(&mut self, node: &Node, style: &Style) {
		let theme = self.env.theme;
		match node {
			Node::Text(text) => self.inline.push_text(text, style),
			Node::Bold(children) => {
				let next = Self::styled(style, |h| h.font_weight = Some(FontWeight::BOLD));
				self.children(children, &next);
			}
			Node::Italic(children) => {
				let next = Self::styled(style, |h| h.font_style = Some(FontStyle::Italic));
				self.children(children, &next);
			}
			Node::Underline(children) => {
				let next = Self::styled(style, |h| {
					h.underline = Some(UnderlineStyle {
						thickness: px(1.),
						color: None,
						wavy: false,
					});
				});
				self.children(children, &next);
			}
			Node::Strike(children) => {
				let next = Self::styled(style, |h| {
					h.strikethrough = Some(StrikethroughStyle {
						thickness: px(1.),
						color: None,
					});
				});
				self.children(children, &next);
			}
			Node::Spoiler(children) => {
				let index = self.env.next_spoiler();
				let revealed = self.env.is_revealed(index);
				let mut next = Self::styled(style, |h| {
					if revealed {
						h.background_color = Some(theme.modifier_active);
					} else {
						h.color = Some(theme.spoiler);
						h.background_color = Some(theme.spoiler);
					}
				});
				next.spoiler = (!revealed).then_some(index);
				self.children(children, &next);
			}
			Node::Code(code) => {
				let start = self.inline.text.len();
				let next = Self::styled(style, |h| h.background_color = Some(theme.code_background));
				self.inline.push(code, &next);
				self.inline.codes.push(start..self.inline.text.len());
			}
			Node::UserMention(id) => {
				let label = format!("@{}", self.env.lookup.user(*id));
				let mut next = style.clone();
				next.user = Some(Snowflake::from(*id));
				self.mention(label, None, &next);
			}
			Node::RoleMention(id) => {
				let (name, color) = self
					.env
					.lookup
					.role(*id)
					.unwrap_or_else(|| (id.to_string(), None));
				self.mention(format!("@{name}"), color.map(crate::model::role_color), style);
			}
			Node::ChannelMention(id) => {
				let name = self.env.lookup.channel(*id).unwrap_or_else(|| id.to_string());
				self.mention(format!("#{name}"), None, style);
			}
			Node::Everyone => self.mention("@everyone".to_owned(), None, style),
			Node::Here => self.mention("@here".to_owned(), None, style),
			Node::CommandMention { name, .. } => self.mention(format!("/{name}"), None, style),
			Node::CustomEmoji { id, animated, name } => self.inline.emoji.push(EmojiAt {
				at: self.inline.text.len(),
				glyph: Glyph::Emoji {
					id: *id,
					animated: *animated,
					name: name.clone(),
				},
			}),
			Node::InlineImage {
				src, width, height, ..
			} => self.inline.emoji.push(EmojiAt {
				at: self.inline.text.len(),
				glyph: Glyph::Image {
					url: src.clone().into(),
					width: *width as f32,
					height: *height as f32,
				},
			}),
			Node::Styled {
				children,
				color,
				background,
				mono,
			} => {
				let next = Self::styled(style, |h| {
					if let Some(color) = color.as_ref().and_then(|color| tint(color, theme)) {
						h.color = Some(color);
					}
					if let Some(fill) = background.as_ref().and_then(|fill| tint(fill, theme)) {
						h.background_color = Some(fill);
					}
				});
				let start = self.inline.text.len();
				self.children(children, &next);
				if *mono {
					self.inline.codes.push(start..self.inline.text.len());
				}
			}
			Node::Timestamp { unix, style: kind } => {
				let label = timestamp_label(*unix, *kind, self.env.prefs.time_24h, self.cx);
				let next = Self::styled(style, |h| h.background_color = Some(theme.code_background));
				self.inline.push(&label, &next);
			}
			Node::Link { url, text, .. } => {
				let mut next = Self::styled(style, |h| h.color = Some(theme.link));
				next.link = Some(url.clone());
				match text {
					Some(children) => self.children(children, &next),
					None => self.inline.push(url, &next),
				}
			}
			Node::GuildNav(kind) => {
				let label = self.cx.t(match kind {
					GuildNavKind::Customize => "guild-nav-customize",
					GuildNavKind::Browse => "guild-nav-browse",
					GuildNavKind::Guide => "guild-nav-guide",
					GuildNavKind::LinkedRoles => "guild-nav-linked-roles",
				});
				self.mention(label.to_string(), None, style);
			}
			Node::LineBreak => self.inline.push("\n", style),
			_ => {}
		}
	}

	fn block(&mut self, node: &Node) {
		let theme = self.env.theme;
		let element = match node {
			Node::CodeBlock { lang, code } => self.code_block(lang.as_deref().unwrap_or_default(), code),
			Node::Heading { level, children } => {
				let size = self.env.scaled(match level {
					1 => theme.font_xl,
					2 => theme.font_lg,
					_ => theme.font_md,
				});
				div()
					.mt(theme.space_8)
					.text_size(size)
					.line_height(size * 1.25)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(self.inline_element(children))
					.into_any_element()
			}
			Node::Subtext(children) => div()
				.text_size(self.env.scaled(theme.font_tiny))
				.line_height(rem(16.))
				.text_color(theme.text_muted)
				.child(self.inline_element(children))
				.into_any_element(),
			Node::Quote(children) => div()
				.flex()
				.gap(theme.space_8)
				.child(
					div()
						.flex_none()
						.w(rem(4.))
						.rounded_full()
						.bg(theme.interactive_muted),
				)
				.child(div().flex().flex_col().min_w_0().children(self.nested(children)))
				.into_any_element(),
			Node::List {
				ordered,
				start,
				items,
			} => div()
				.flex()
				.flex_col()
				.pl(theme.space_16)
				.children(items.iter().enumerate().map(|(index, item)| {
					let marker = if *ordered {
						format!("{}.", *start as usize + index)
					} else {
						"\u{2022}".to_owned()
					};
					div()
						.flex()
						.gap(theme.space_8)
						.child(div().flex_none().text_color(theme.text_muted).child(marker))
						.child(div().flex().flex_col().min_w_0().children(self.nested(item)))
				}))
				.into_any_element(),
			_ => return,
		};
		self.out.push(element);
	}

	fn code_block(&self, language: &str, code: &str) -> AnyElement {
		let theme = self.env.theme;
		let index = self.env.next_code_block();
		if let Some(block) = self.env.patch.and_then(|patch| patch.code_block(index)) {
			let (text, runs) = plugin_runs(block, theme);
			return self.code_frame(text, runs);
		}
		let code = code.trim_end().replace('\t', "    ");
		let mut runs: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
		let mut covered = 0;
		for span in highlight(language, &code) {
			if span.start < covered || span.kind == TokenKind::Plain {
				continue;
			}
			let color = match span.kind {
				TokenKind::Keyword => theme.mention_text,
				TokenKind::String => theme.success,
				TokenKind::Comment => theme.text_muted,
				TokenKind::Number => theme.warning,
				TokenKind::Type => theme.link,
				_ => theme.text_normal,
			};
			covered = span.end;
			runs.push((
				span.start..span.end,
				HighlightStyle {
					color: Some(color),
					..HighlightStyle::default()
				},
			));
		}
		self.code_frame(code, runs)
	}

	fn code_frame(&self, code: String, runs: Vec<(Range<usize>, HighlightStyle)>) -> AnyElement {
		let theme = self.env.theme;
		div()
			.mt(theme.space_4)
			.px(theme.space_12)
			.py(theme.space_8)
			.rounded(theme.radius_sm)
			.bg(theme.code_background)
			.border_1()
			.border_color(theme.code_border)
			.font_family(theme.code_font.clone())
			.text_size(self.env.scaled(theme.font_sm))
			.line_height(rem(18.) * self.env.prefs.font_scale)
			.child({
				let text = SharedString::from(code);
				self.env
					.selectable(text.clone(), StyledText::new(text).with_highlights(runs))
			})
			.into_any_element()
	}
}

fn plugin_runs(block: &CodeBlockNode, theme: &Theme) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
	let mut text = String::new();
	let mut runs = Vec::new();
	for (index, line) in block.lines.iter().enumerate() {
		if index > 0 {
			text.push('\n');
		}
		for span in line {
			let start = text.len();
			text.push_str(&span.text);
			let style = HighlightStyle {
				color: span.color.as_ref().and_then(|color| resolve_color(color, theme)),
				font_weight: span.bold.then_some(FontWeight::BOLD),
				font_style: span.italic.then_some(FontStyle::Italic),
				..HighlightStyle::default()
			};
			if start < text.len() && style != HighlightStyle::default() {
				runs.push((start..text.len(), style));
			}
		}
	}
	(text, runs)
}

fn timestamp_label(unix: i64, style: TimestampStyle, h24: bool, cx: &App) -> String {
	let pattern = match (h24, style) {
		(true, TimestampStyle::ShortTime) => "%H:%M",
		(true, TimestampStyle::LongTime) => "%H:%M:%S",
		(true, TimestampStyle::ShortDateTime) => "%B %-d, %Y %H:%M",
		(true, TimestampStyle::LongDateTime) => "%A, %B %-d, %Y %H:%M",
		_ => return format::timestamp(unix, style, cx),
	};
	DateTime::from_timestamp(unix, 0).map_or_else(
		|| unix.to_string(),
		|time| time.with_timezone(&Local).format(pattern).to_string(),
	)
}

pub fn snippet(document: &Document, limit: usize) -> String {
	let flat = document
		.plain_text()
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	flat.chars().take(limit).collect()
}
