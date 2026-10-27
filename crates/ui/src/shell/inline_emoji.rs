use std::ops::Range;

use gpui::{
	AnyElement, App, Context, Entity, InteractiveElement as _, IntoElement as _, ParentElement as _,
	SharedString, Styled as _, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::input::{InlineToken, InlineTokenContext, InputContent, TextareaState};
use media::SizeHint;

use crate::images::{Images, frame_id};
use crate::theme::{Theme, with_alpha};

const CUSTOM_PIXELS: u32 = 48;
const MAX_MARKUP: usize = 128;
const SELECTED_ALPHA: f32 = 0.4;

fn is_name(name: &str) -> bool {
	!name.is_empty()
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn custom_token(markup: &str) -> Option<InlineToken> {
	let inner = markup.strip_prefix('<')?.strip_suffix('>')?;
	let (animated, body) = match inner.strip_prefix("a:") {
		Some(body) => (true, body),
		None => (false, inner.strip_prefix(':')?),
	};
	let (name, id) = body.split_once(':')?;
	let id: u64 = id.parse().ok()?;
	if !is_name(name) {
		return None;
	}
	let url = media::cdn::emoji_url(id, animated, CUSTOM_PIXELS);
	Some(InlineToken::new(url, markup).with_label(format!(":{name}:")))
}

fn custom_spans(text: &str) -> Vec<(Range<usize>, InlineToken)> {
	let mut found = Vec::new();
	let mut search = 0;
	while let Some(offset) = text[search..].find('<') {
		let start = search + offset;
		let window = &text[start..text.len().min(start + MAX_MARKUP)];
		let token = window
			.find('>')
			.and_then(|close| Some((close, custom_token(&window[..=close])?)));
		match token {
			Some((close, token)) => {
				found.push((start..start + close + 1, token));
				search = start + close + 1;
			}
			None => search = start + 1,
		}
	}
	found
}

pub fn spans(text: &str) -> Vec<(Range<usize>, InlineToken)> {
	let mut found = if text.contains('<') {
		custom_spans(text)
	} else {
		Vec::new()
	};
	found.extend(
		media::twemoji::find(text)
			.into_iter()
			.map(|(range, url)| (range.clone(), InlineToken::new(url, &text[range]))),
	);
	found.sort_by_key(|(range, _)| range.start);
	found
}

pub fn content(text: impl Into<SharedString>) -> InputContent {
	let text = text.into();
	spans(&text)
		.into_iter()
		.fold(InputContent::new(text.clone()), |content, (range, token)| {
			content.clone().with_token(range, token).unwrap_or(content)
		})
}

pub fn insert(state: &mut TextareaState, text: &str, window: &mut Window, cx: &mut Context<TextareaState>) {
	let mut cursor = 0;
	for (range, token) in spans(text) {
		if cursor < range.start {
			state.replace(text[cursor..range.start].to_owned(), window, cx);
		}
		if state.replace_with_token(token, window, cx).is_err() {
			state.replace(text[range.clone()].to_owned(), window, cx);
		}
		cursor = range.end;
	}
	if cursor < text.len() {
		state.replace(text[cursor..].to_owned(), window, cx);
	}
}

#[derive(Clone, Default)]
pub struct EmojiTokens {
	settled: Option<SharedString>,
}

impl EmojiTokens {
	pub fn sync(&mut self, input: &Entity<TextareaState>, window: &mut Window, cx: &mut App) {
		let (text, missing, selection) = {
			let state = input.read(cx);
			let text = state.value();
			if (text.is_ascii() && !text.contains('<')) || self.settled.as_ref() == Some(&text) {
				return;
			}
			let taken = state.tokens();
			let missing: Vec<_> = spans(&text)
				.into_iter()
				.filter(|(range, _)| {
					!taken.iter().any(|span| {
						let held = span.range();
						held.start < range.end && range.start < held.end
					})
				})
				.collect();
			(text, missing, state.selected_range())
		};
		if missing.is_empty() {
			return;
		}
		self.settled = Some(text);
		input.update(cx, |state, cx| {
			for (range, token) in missing.into_iter().rev() {
				if state.replace_range_with_token(range, token, window, cx).is_err() {
					break;
				}
			}
			state.set_selected_range(selection, cx);
		});
	}
}

pub fn render(context: &InlineTokenContext, _: &mut Window, cx: &mut App) -> AnyElement {
	let side = context.line_height();
	let url = context.token().id();
	let pixels = (side.as_f32() * 2.) as u32;
	let image = cx.global::<Images>().get(url, SizeHint::square(pixels));
	let theme = cx.global::<Theme>();
	div()
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size(side)
		.rounded(theme.radius_sm)
		.when(context.is_selected(), |cell| {
			cell.bg(with_alpha(theme.brand, SELECTED_ALPHA))
		})
		.children(image.map(|image| img(image).id(frame_id(url)).size(side * 0.9)))
		.into_any_element()
}
