use std::rc::Rc;

use gpui::{
	AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _,
	Rems, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Gif, GifCategory};
use session::Command;

use super::{Picker, PickerEvent};
use crate::components::tooltip;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::community::art::cover;
use crate::theme::{Theme, rem, with_alpha};

const MIN_TILE: f32 = 64.;
const MAX_TILE: f32 = 240.;
const CATEGORY_HEIGHT: f32 = 104.;
const MIN_SUGGEST: usize = 2;
const STAR_SIZE: f32 = 26.;

type ChipHandler = Rc<dyn Fn(&mut Picker, &mut Window, &mut Context<Picker>)>;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum GifView {
	#[default]
	Trending,
	Favorites,
}

#[derive(Default)]
pub struct GifState {
	active: Option<String>,
	hovered: Option<String>,
	view: GifView,
	categories_open: bool,
	categories_pending: bool,
	favorites_pending: bool,
	search_pending: bool,
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
		let missing = self.data.read(cx).state().gif_trending().is_empty();
		if missing {
			self.send(Command::LoadGifTrending, cx);
		}
	}

	pub(super) fn reset_gifs(&mut self) {
		self.gif.active = None;
		self.gif.view = GifView::Trending;
		self.gif.hovered = None;
		self.gif.categories_pending = false;
		self.gif.favorites_pending = false;
		self.gif.search_pending = false;
	}

	pub(super) fn open_categories(&mut self, cx: &mut Context<Self>) {
		self.gif.categories_open = true;
		if !self.gif.categories_pending && self.data.read(cx).state().gif_categories().is_none() {
			self.gif.categories_pending = true;
			self.send(Command::LoadGifCategories, cx);
		}
		cx.notify();
	}

	pub(super) fn open_favorites(&mut self, cx: &mut Context<Self>) {
		self.gif.view = GifView::Favorites;
		self.gif.active = None;
		if !self.gif.favorites_pending && self.data.read(cx).state().gif_favorites().is_none() {
			self.gif.favorites_pending = true;
			self.send(Command::LoadGifFavorites, cx);
		}
		cx.notify();
	}

	pub(super) fn back_to_trending(&mut self, cx: &mut Context<Self>) {
		self.gif.view = GifView::Trending;
		cx.notify();
	}

	pub(super) fn toggle_favorite(&mut self, gif: Gif, cx: &mut Context<Self>) {
		let favorited = self
			.data
			.read(cx)
			.state()
			.gif_favorites()
			.is_some_and(|favorites| favorites.iter().any(|favorite| favorite.id == gif.id));
		if favorited {
			self.send(Command::UnfavoriteGif { id: gif.id.clone() }, cx);
		} else {
			self.send(Command::FavoriteGif { id: gif.id.clone() }, cx);
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
		self.gif.search_pending = !known;
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

	fn is_favorited(&self, gif: &Gif, cx: &Context<Self>) -> bool {
		self.data
			.read(cx)
			.state()
			.gif_favorites()
			.is_some_and(|favorites| favorites.iter().any(|favorite| favorite.id == gif.id))
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
		let star = gif.clone();
		let favorited = self.is_favorited(gif, cx);
		let tile = div()
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
			.when(hovered, |tile| {
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
						.on_click(cx.listener(move |this, _, _, cx| this.toggle_favorite(star.clone(), cx)))
						.child(Icon::new(IconKind::Star).size(rem(14.)).color(if favorited {
							theme.warning
						} else {
							theme.interactive_muted
						})),
				)
			});
		tile.into_any_element()
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

	fn chip(
		&self,
		id: &str,
		label: SharedString,
		handler: ChipHandler,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let handler = handler.clone();
		div()
			.id(format!("{}-{}", id, label))
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
			.on_click(cx.listener(move |this, _, window, cx| handler(this, window, cx)))
			.child(label)
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

	fn categories_chip(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		self.chip(
			"gif-show-categories",
			cx.t("picker-gif-show-categories"),
			Rc::new(move |this, _, cx| this.open_categories(cx)),
			theme,
			cx,
		)
	}

	fn categories_grid(
		&self,
		categories: &[GifCategory],
		column: Rems,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		if !self.gif.categories_open {
			return None;
		}
		let body = if categories.is_empty() {
			self.loading_caption(cx.t("picker-gif-loading"), theme)
		} else {
			let tiles: Vec<AnyElement> = categories
				.iter()
				.enumerate()
				.map(|(index, category)| self.category_tile(index, category, column, theme, cx))
				.collect();
			div()
				.flex()
				.flex_wrap()
				.gap(theme.gif_gap)
				.children(tiles)
				.into_any_element()
		};
		Some(
			div()
				.flex()
				.flex_col()
				.gap(theme.space_8)
				.child(self.heading(cx.t("picker-gif-categories"), theme))
				.child(body)
				.into_any_element(),
		)
	}

	fn masonry_or_empty(
		&self,
		gifs: &[Gif],
		empty_text: SharedString,
		column: Rems,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(self.masonry(gifs, column, theme, cx))
			.when(gifs.is_empty(), |view| {
				view.child(self.empty_state(empty_text, theme))
			})
			.into_any_element()
	}

	pub(super) fn gif_body(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		{
			let state = self.data.read(cx).state();
			if state.gif_categories().is_some() {
				self.gif.categories_pending = false;
			}
			if state.gif_favorites().is_some() {
				self.gif.favorites_pending = false;
			}
			if let Some(query) = self.gif.active.as_deref()
				&& state.gif_search(query).is_some()
			{
				self.gif.search_pending = false;
			}
		}
		let column = (theme.picker_width - theme.space_12 * 2. - theme.space_8 - theme.gif_gap) / 2.;
		let (categories, trending, favorites, results, suggestions) = {
			let data = self.data.read(cx);
			let state = data.state();
			(
				state.gif_categories().map(<[_]>::to_vec).unwrap_or_default(),
				state.gif_trending().to_vec(),
				state.gif_favorites().map(<[_]>::to_vec),
				self.gif
					.active
					.as_deref()
					.and_then(|query| state.gif_search(query).map(<[Gif]>::to_vec)),
				state.gif_suggestions(&self.query).map(<[String]>::to_vec),
			)
		};
		let content: AnyElement = if let Some(found) = results {
			self.masonry_or_empty(&found, cx.t("picker-gif-empty"), column, theme, cx)
		} else if self.gif.active.is_some() {
			self.loading_caption(cx.t("picker-gif-loading"), theme)
		} else if self.gif.view == GifView::Favorites {
			let body = match favorites {
				Some(gifs) => {
					self.masonry_or_empty(&gifs, cx.t("picker-gif-favorites-empty"), column, theme, cx)
				}
				None => self.loading_caption(cx.t("picker-gif-loading"), theme),
			};
			div()
				.flex()
				.flex_col()
				.gap(theme.space_8)
				.child(self.chip(
					"gif-back-trending",
					cx.t("picker-gif-trending"),
					Rc::new(move |this, _, cx| this.back_to_trending(cx)),
					theme,
					cx,
				))
				.child(self.heading(cx.t("picker-gif-favorites"), theme))
				.child(body)
				.into_any_element()
		} else {
			let suggestions_view = suggestions
				.as_ref()
				.filter(|_| !self.query.is_empty())
				.map(|terms| self.suggestion_chips(terms, theme, cx));
			let mut parts: Vec<AnyElement> = Vec::new();
			if let Some(view) = suggestions_view {
				parts.push(view);
			}
			parts.push(
				div()
					.flex()
					.flex_row()
					.items_center()
					.gap(theme.space_8)
					.child(self.categories_chip(theme, cx))
					.child(self.chip(
						"gif-favorites",
						cx.t("picker-gif-favorites"),
						Rc::new(move |this, _, cx| this.open_favorites(cx)),
						theme,
						cx,
					))
					.into_any_element(),
			);
			if let Some(grid) = self.categories_grid(&categories, column, theme, cx) {
				parts.push(grid);
			}
			parts.push(self.heading(cx.t("picker-gif-trending"), theme));
			parts.push(if trending.is_empty() {
				self.loading_caption(cx.t("picker-gif-loading"), theme)
			} else {
				self.masonry_or_empty(&trending, cx.t("picker-gif-empty"), column, theme, cx)
			});
			div()
				.flex()
				.flex_col()
				.gap(theme.space_8)
				.children(parts)
				.into_any_element()
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
