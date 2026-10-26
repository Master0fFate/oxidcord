use std::collections::HashSet;

use gpui::{
	AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _,
	Rems, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use media::cdn::{GifTile, gif_tile};
use protocol::{Gif, GifCategory};
use session::Command;

use super::{Picker, PickerEvent};
use crate::components::tooltip;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::images::scaled_pixels;
use crate::shell::community::art::{cover_animated, cover_still};
use crate::theme::{Theme, rem, rem_to_pixels, with_alpha};

const MIN_TILE: f32 = 64.;
const MAX_TILE: f32 = 240.;
const CATEGORY_HEIGHT: f32 = 104.;
const MIN_SUGGEST: usize = 2;
const STAR_SIZE: f32 = 26.;

#[derive(Default)]
pub struct GifState {
	active: Option<String>,
	hovered: Option<String>,
	favorites: bool,
}

fn pixels(measure: Rems) -> u32 {
	scaled_pixels(rem_to_pixels(measure).round() as u32)
}

fn tile_media(gif: &Gif, width: Rems, height: Rems) -> GifTile {
	let source = if gif.gif_src.is_empty() {
		&gif.src
	} else {
		&gif.gif_src
	};
	gif_tile(source, &gif.preview, pixels(width), pixels(height))
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
		if self.data.read(cx).state().gif_trending().is_none() {
			self.send(Command::LoadGifTrending, cx);
		}
		self.load_favorites(cx);
	}

	pub(super) fn reset_gifs(&mut self) {
		self.gif.active = None;
		self.gif.hovered = None;
		self.gif.favorites = false;
	}

	fn load_favorites(&mut self, cx: &mut Context<Self>) {
		if self.data.read(cx).state().gif_favorites().is_none() {
			self.send(Command::LoadGifFavorites, cx);
		}
	}

	fn open_favorites(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.gif.favorites = true;
		self.gif.active = None;
		self.set_query(String::new(), window, cx);
		self.load_favorites(cx);
		cx.notify();
	}

	fn toggle_favorites(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.gif.favorites {
			self.gif.favorites = false;
			self.set_query(String::new(), window, cx);
			cx.notify();
		} else {
			self.open_favorites(window, cx);
		}
	}

	pub(super) fn toggle_favorite(&mut self, gif: Gif, cx: &mut Context<Self>) {
		if self.is_favorited(&gif, cx) {
			self.send(Command::UnfavoriteGif { url: gif.url }, cx);
		} else {
			self.send(Command::FavoriteGif { gif }, cx);
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
		if self.share_usage && gif.id != gif.url {
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

	fn is_favorited(&self, gif: &Gif, cx: &Context<Self>) -> bool {
		self.data
			.read(cx)
			.state()
			.gif_favorites()
			.is_some_and(|favorites| favorites.iter().any(|favorite| favorite.url == gif.url))
	}

	fn gif_tile(
		&self,
		gif: &Gif,
		width: Rems,
		height: Rems,
		favorited: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let hovered = self.gif.hovered.as_deref() == Some(gif.id.as_str());
		let media = tile_media(gif, width, height);
		let id = gif.id.clone();
		let hover_id = gif.id.clone();
		let picked = gif.clone();
		let star = gif.clone();
		div()
			.id(SharedString::from(format!("gif-{}", gif.id)))
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
			.child(match media.animated.as_deref().filter(|_| hovered) {
				Some(animated) => cover_animated(animated, &media.still, width, height, ObjectFit::Cover, cx),
				None => cover_still(&media.still, width, height, ObjectFit::Cover, cx),
			})
			.when(hovered || favorited, |tile| {
				tile.child(
					div()
						.id(format!("gif-star-{}", gif.id))
						.absolute()
						.top(theme.space_4)
						.right(theme.space_4)
						.size(rem(STAR_SIZE))
						.rounded_full()
						.bg(with_alpha(theme.background_floating, 0.85))
						.flex()
						.items_center()
						.justify_center()
						.cursor_pointer()
						.tooltip(tooltip(cx.t(if favorited {
							"picker-gif-unfavorite"
						} else {
							"picker-gif-favorite"
						})))
						.on_click(cx.listener(move |this, _, _, cx| {
							cx.stop_propagation();
							this.toggle_favorite(star.clone(), cx);
						}))
						.child(Icon::new(IconKind::Star).size(rem(14.)).color(if favorited {
							theme.warning
						} else {
							theme.interactive_muted
						})),
				)
			})
			.into_any_element()
	}

	fn masonry(
		&self,
		gifs: &[Gif],
		favorited: &HashSet<&str>,
		column: Rems,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut left: Vec<AnyElement> = Vec::new();
		let mut right: Vec<AnyElement> = Vec::new();
		let (mut left_height, mut right_height) = (0_f32, 0_f32);
		for gif in gifs {
			let height = tile_height(gif, column);
			let tile = self.gif_tile(
				gif,
				column,
				height,
				favorited.contains(gif.url.as_str()),
				theme,
				cx,
			);
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

	fn favorites_tile(&self, width: Rems, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let active = self.gif.favorites;
		div()
			.id("gif-favorites-category")
			.relative()
			.flex_none()
			.w(width)
			.h(rem(CATEGORY_HEIGHT))
			.rounded(theme.radius_sm)
			.overflow_hidden()
			.cursor_pointer()
			.bg(theme.background_secondary)
			.when(active, |tile| tile.border_2().border_color(theme.brand))
			.hover(|style| style.bg(theme.modifier_selected))
			.flex()
			.items_center()
			.justify_center()
			.gap(theme.space_8)
			.on_click(cx.listener(|this, _, window, cx| this.toggle_favorites(window, cx)))
			.child(Icon::new(IconKind::Star).size(theme.icon_md).color(if active {
				theme.warning
			} else {
				theme.interactive_muted
			}))
			.child(
				div()
					.text_size(theme.font_md)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(cx.t("picker-gif-favorites")),
			)
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
			.child(cover_still(
				&gif_tile(&category.src, "", pixels(width), pixels(rem(CATEGORY_HEIGHT))).still,
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

	fn heading(&self, text: SharedString, theme: &Theme) -> AnyElement {
		div()
			.pt(theme.space_8)
			.pb(theme.space_4)
			.text_size(theme.font_tiny)
			.font_weight(FontWeight::BOLD)
			.text_color(theme.header_secondary)
			.child(text.to_uppercase())
			.into_any_element()
	}

	fn empty_state(&self, text: SharedString, theme: &Theme) -> AnyElement {
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
			.child(text)
			.into_any_element()
	}

	fn loading_caption(&self, text: SharedString, theme: &Theme) -> AnyElement {
		div()
			.flex()
			.items_center()
			.p(theme.space_16)
			.text_size(theme.font_sm)
			.text_color(theme.text_muted)
			.child(text)
			.into_any_element()
	}

	fn masonry_or_empty(
		&self,
		gifs: &[Gif],
		favorited: &HashSet<&str>,
		empty_text: SharedString,
		column: Rems,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if gifs.is_empty() {
			return self.empty_state(empty_text, theme);
		}
		self.masonry(gifs, favorited, column, theme, cx)
	}

	fn tile_row(&self, tiles: Vec<AnyElement>, theme: &Theme) -> AnyElement {
		div()
			.flex()
			.flex_wrap()
			.gap(theme.gif_gap)
			.children(tiles)
			.into_any_element()
	}

	pub(super) fn gif_body(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let column = (theme.picker_width - theme.space_12 * 2. - theme.space_8 - theme.gif_gap) / 2.;
		let (trending, favorites, results, suggestions) = {
			let state = self.data.read(cx).state();
			(
				state.gif_trending().cloned(),
				state.gif_favorites().cloned(),
				self.gif
					.active
					.as_deref()
					.map(|query| state.gif_search(query).cloned()),
				state
					.gif_suggestions(&self.query)
					.filter(|_| !self.query.is_empty())
					.map(<[String]>::to_vec),
			)
		};
		let favorited: HashSet<&str> = favorites
			.iter()
			.flat_map(|gifs| gifs.iter())
			.map(|gif| gif.url.as_str())
			.collect();
		let content = match results {
			Some(Some(found)) => {
				self.masonry_or_empty(&found, &favorited, cx.t("picker-gif-empty"), column, theme, cx)
			}
			Some(None) => self.loading_caption(cx.t("picker-gif-loading"), theme),
			None if self.gif.favorites => {
				let body = match &favorites {
					Some(gifs) => self.masonry_or_empty(
						gifs,
						&favorited,
						cx.t("picker-gif-favorites-empty"),
						column,
						theme,
						cx,
					),
					None => self.loading_caption(cx.t("picker-gif-loading"), theme),
				};
				let row = self.tile_row(vec![self.favorites_tile(column, theme, cx)], theme);
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(row)
					.child(self.heading(cx.t("picker-gif-favorites"), theme))
					.child(body)
					.into_any_element()
			}
			None => {
				let mut tiles = vec![self.favorites_tile(column, theme, cx)];
				let gifs =
					match &trending {
						Some(trending) => {
							tiles.extend(trending.categories.iter().enumerate().map(|(index, category)| {
								self.category_tile(index, category, column, theme, cx)
							}));
							self.masonry_or_empty(
								&trending.gifs,
								&favorited,
								cx.t("picker-gif-empty"),
								column,
								theme,
								cx,
							)
						}
						None => self.loading_caption(cx.t("picker-gif-loading"), theme),
					};
				let row = self.tile_row(tiles, theme);
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.children(suggestions.map(|terms| self.suggestion_chips(&terms, theme, cx)))
					.child(self.heading(cx.t("picker-gif-categories"), theme))
					.child(row)
					.child(self.heading(cx.t("picker-gif-trending"), theme))
					.child(gifs)
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
