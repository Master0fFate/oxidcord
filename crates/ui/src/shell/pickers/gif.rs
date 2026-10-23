use gpui::{
	AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _,
	Rems, SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Gif, GifCategory};
use session::Command;

use super::{Picker, PickerEvent};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::community::art::cover;
use crate::theme::{Theme, rem, with_alpha};

const MIN_TILE: f32 = 64.;
const MAX_TILE: f32 = 240.;
const CATEGORY_HEIGHT: f32 = 104.;
const MIN_SUGGEST: usize = 2;

#[derive(Default)]
pub struct GifState {
	active: Option<String>,
	hovered: Option<String>,
}

fn tile_height(gif: &Gif, column: Rems) -> Rems {
	let ratio = if gif.width == 0 {
		1.
	} else {
		gif.height as f32 / gif.width as f32
	};
	rem((column.0 * 16. * ratio).clamp(MIN_TILE, MAX_TILE))
}

impl Picker {
	pub(super) fn load_gifs(&mut self, cx: &mut Context<Self>) {
		let missing = self.data.read(cx).state().gif_trending().is_none();
		if missing {
			self.send(Command::LoadGifTrending, cx);
		}
	}

	pub(super) fn gif_query_changed(&mut self, cx: &mut Context<Self>) {
		if self.query.is_empty() {
			self.gif.active = None;
			return;
		}
		if self.query.chars().count() < MIN_SUGGEST {
			return;
		}
		let known = self.data.read(cx).state().gif_suggestions(&self.query).is_some();
		if !known {
			self.send(
				Command::SuggestGifs {
					query: self.query.clone(),
				},
				cx,
			);
		}
	}

	pub(super) fn submit_gif_search(&mut self, query: String, cx: &mut Context<Self>) {
		if query.is_empty() {
			self.gif.active = None;
			cx.notify();
			return;
		}
		let known = self.data.read(cx).state().gif_search(&query).is_some();
		if !known {
			self.send(Command::SearchGifs { query: query.clone() }, cx);
		}
		self.gif.active = Some(query);
		cx.notify();
	}

	fn pick_gif(&mut self, gif: Gif, cx: &mut Context<Self>) {
		if self.share_usage {
			self.send(
				Command::SelectGif {
					id: gif.id.clone(),
					query: self.gif.active.clone().unwrap_or_default(),
				},
				cx,
			);
		}
		self.send(
			Command::SendGif {
				channel_id: self.channel,
				gif,
			},
			cx,
		);
		cx.emit(PickerEvent::Sent);
	}

	fn gif_tile(&self, gif: &Gif, width: Rems, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let height = tile_height(gif, width);
		let hovered = self.gif.hovered.as_deref() == Some(gif.id.as_str());
		let still = if gif.preview.is_empty() {
			&gif.gif_src
		} else {
			&gif.preview
		};
		let url = if hovered && !gif.gif_src.is_empty() {
			&gif.gif_src
		} else {
			still
		};
		let id = gif.id.clone();
		let hover_id = gif.id.clone();
		let picked = gif.clone();
		div()
			.id(gpui::SharedString::from(format!("gif-{}", gif.id)))
			.relative()
			.flex_none()
			.w(width)
			.h(height)
			.rounded(theme.radius_sm)
			.overflow_hidden()
			.cursor_pointer()
			.bg(theme.background_secondary)
			.when(hovered, |tile| tile.border_2().border_color(theme.brand))
			.on_hover(cx.listener(move |this, over: &bool, _, cx| {
				if *over {
					this.gif.hovered = Some(hover_id.clone());
				} else if this.gif.hovered.as_deref() == Some(id.as_str()) {
					this.gif.hovered = None;
				}
				cx.notify();
			}))
			.on_click(cx.listener(move |this, _, _, cx| this.pick_gif(picked.clone(), cx)))
			.child(cover(url, width, height, ObjectFit::Cover, cx))
			.into_any_element()
	}

	fn masonry(&self, gifs: &[Gif], column: Rems, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let mut left: Vec<AnyElement> = Vec::new();
		let mut right: Vec<AnyElement> = Vec::new();
		let (mut left_height, mut right_height) = (0_f32, 0_f32);
		for gif in gifs {
			let height = tile_height(gif, column);
			let tile = self.gif_tile(gif, column, theme, cx);
			if left_height <= right_height {
				left.push(tile);
				left_height += height.0 + theme.gif_gap.0;
			} else {
				right.push(tile);
				right_height += height.0 + theme.gif_gap.0;
			}
		}
		div()
			.flex()
			.gap(theme.gif_gap)
			.child(div().flex().flex_col().gap(theme.gif_gap).children(left))
			.child(div().flex().flex_col().gap(theme.gif_gap).children(right))
			.into_any_element()
	}

	fn category_tile(
		&self,
		index: usize,
		category: &GifCategory,
		width: Rems,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let name = category.name.clone();
		div()
			.id(("gif-category", index))
			.relative()
			.flex_none()
			.w(width)
			.h(rem(CATEGORY_HEIGHT))
			.rounded(theme.radius_sm)
			.overflow_hidden()
			.cursor_pointer()
			.on_click(cx.listener(move |this, _, window, cx| {
				this.set_query(name.to_lowercase(), window, cx);
				this.submit_gif_search(name.to_lowercase(), cx);
			}))
			.child(cover(
				&category.src,
				width,
				rem(CATEGORY_HEIGHT),
				ObjectFit::Cover,
				cx,
			))
			.child(
				div()
					.absolute()
					.inset_0()
					.flex()
					.items_center()
					.justify_center()
					.bg(with_alpha(theme.background_tertiary, 0.55))
					.text_size(theme.font_md)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(category.name.clone()),
			)
			.into_any_element()
	}

	fn suggestion_chips(&self, terms: &[String], theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		div()
			.flex()
			.flex_wrap()
			.gap(theme.space_8)
			.children(terms.iter().take(8).enumerate().map(|(index, term)| {
				let query = term.to_lowercase();
				div()
					.id(("gif-suggestion", index))
					.flex()
					.items_center()
					.h(rem(28.))
					.px(theme.space_12)
					.rounded_full()
					.cursor_pointer()
					.bg(theme.background_secondary)
					.hover(|style| style.bg(theme.modifier_selected))
					.text_size(theme.font_sm)
					.text_color(theme.interactive_normal)
					.on_click(cx.listener(move |this, _, window, cx| {
						this.set_query(query.clone(), window, cx);
						this.submit_gif_search(query.clone(), cx);
					}))
					.child(term.clone())
			}))
			.into_any_element()
	}

	pub(super) fn gif_body(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let column = (theme.picker_width - theme.space_12 * 2. - theme.space_8 - theme.gif_gap) / 2.;
		let (categories, trending, results, suggestions) = {
			let data = self.data.read(cx);
			let state = data.state();
			let trending = state.gif_trending();
			(
				trending
					.map(|trending| trending.categories.to_vec())
					.unwrap_or_default(),
				trending
					.map(|trending| trending.gifs.to_vec())
					.unwrap_or_default(),
				self.gif
					.active
					.as_deref()
					.map(|query| state.gif_search(query).map(<[Gif]>::to_vec)),
				state.gif_suggestions(&self.query).map(<[String]>::to_vec),
			)
		};
		let heading = |text: SharedString| {
			div()
				.pt(theme.space_8)
				.pb(theme.space_4)
				.text_size(theme.font_tiny)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_secondary)
				.child(text.to_uppercase())
		};
		let categories_title = cx.t("picker-gif-categories");
		let trending_title = cx.t("picker-gif-trending");
		let content: AnyElement = match results {
			Some(found) => {
				let gifs = found.unwrap_or_default();
				let empty = gifs.is_empty();
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(self.masonry(&gifs, column, theme, cx))
					.when(empty, |view| {
						view.child(
							div()
								.flex()
								.flex_col()
								.items_center()
								.gap(theme.space_8)
								.p(theme.space_32)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(
									Icon::new(IconKind::Gif)
										.size(theme.portrait_lg)
										.color(theme.interactive_muted),
								)
								.child(cx.t("picker-gif-empty")),
						)
					})
					.into_any_element()
			}
			None => {
				let tiles: Vec<AnyElement> = categories
					.iter()
					.enumerate()
					.map(|(index, category)| self.category_tile(index, category, column, theme, cx))
					.collect();
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.children(
						suggestions
							.as_ref()
							.filter(|_| !self.query.is_empty())
							.map(|terms| self.suggestion_chips(terms, theme, cx)),
					)
					.child(heading(categories_title))
					.child(div().flex().flex_wrap().gap(theme.gif_gap).children(tiles))
					.child(heading(trending_title))
					.child(self.masonry(&trending, column, theme, cx))
					.into_any_element()
			}
		};
		div()
			.flex()
			.flex_col()
			.px(theme.space_12)
			.pb(theme.space_12)
			.child(content)
			.overflow_y_scrollbar()
			.id("gif-scroll")
			.flex_1()
			.min_h_0()
			.into_any_element()
	}
}
