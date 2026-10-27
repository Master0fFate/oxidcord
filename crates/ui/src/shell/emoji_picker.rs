use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, ObjectFit, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
	Styled as _, StyledImage as _, Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use media::SizeHint;

use crate::components::unicode_emoji;
use crate::emoji::{self, CATEGORIES};
use crate::i18n::Translate as _;
use crate::images::Images;
use crate::theme::{Theme, rem};

const WIDTH: f32 = 352.;
const HEIGHT: f32 = 340.;
const CELL: f32 = 40.;
const CUSTOM_PIXELS: u32 = 64;
const SEARCH_LIMIT: usize = 120;

#[derive(Clone, PartialEq)]
pub enum EmojiChoice {
	Unicode(String),
	Custom { name: String, id: u64, animated: bool },
}

impl EmojiChoice {
	pub fn reaction_key(&self) -> String {
		match self {
			Self::Unicode(glyph) => glyph.clone(),
			Self::Custom { name, id, .. } => format!("{name}:{id}"),
		}
	}

	pub fn markup(&self) -> String {
		match self {
			Self::Unicode(glyph) => glyph.clone(),
			Self::Custom { name, id, animated } => {
				let prefix = if *animated { "a" } else { "" };
				format!("<{prefix}:{name}:{id}>")
			}
		}
	}
}

pub enum EmojiPickerEvent {
	Pick(EmojiChoice),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
	Custom,
	Unicode(usize),
}

pub struct EmojiPicker {
	custom: Vec<(String, u64, bool)>,
	tab: Tab,
	query: String,
	search: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<EmojiPickerEvent> for EmojiPicker {}

impl EmojiPicker {
	pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
		let placeholder = cx.t("emoji-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscription = cx.subscribe_in(&search, window, |this, input, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				this.query = input.read(cx).value().trim().to_lowercase();
				cx.notify();
			}
		});
		Self {
			custom: Vec::new(),
			tab: Tab::Unicode(0),
			query: String::new(),
			search,
			_subscription: subscription,
		}
	}

	pub fn open(&mut self, custom: Vec<(String, u64, bool)>, window: &mut Window, cx: &mut Context<Self>) {
		self.tab = if custom.is_empty() {
			Tab::Unicode(0)
		} else {
			Tab::Custom
		};
		self.custom = custom;
		self.query.clear();
		self.search.update(cx, |state, cx| {
			state.set_value("", window, cx);
			state.focus(window, cx);
		});
		cx.notify();
	}

	fn pick(&mut self, choice: EmojiChoice, cx: &mut Context<Self>) {
		cx.emit(EmojiPickerEvent::Pick(choice));
	}

	fn custom_cell(
		&self,
		index: usize,
		name: &str,
		id: u64,
		animated: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let url = media::cdn::emoji_url(id, animated, CUSTOM_PIXELS);
		let image = cx.global::<Images>().get(&url, SizeHint::square(CUSTOM_PIXELS));
		let choice = EmojiChoice::Custom {
			name: name.to_owned(),
			id,
			animated,
		};
		div()
			.id(("emoji-custom", index))
			.flex()
			.items_center()
			.justify_center()
			.size(rem(CELL))
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_selected))
			.on_click(cx.listener(move |this, _, _, cx| this.pick(choice.clone(), cx)))
			.child(
				div()
					.size(rem(CELL - 12.))
					.children(image.map(|image| img(image).size_full().object_fit(ObjectFit::Contain))),
			)
			.into_any_element()
	}

	fn unicode_cell(
		&self,
		index: usize,
		glyph: &'static str,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let choice = EmojiChoice::Unicode(glyph.to_owned());
		div()
			.id(("emoji-unicode", index))
			.flex()
			.items_center()
			.justify_center()
			.size(rem(CELL))
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_selected))
			.on_click(cx.listener(move |this, _, _, cx| this.pick(choice.clone(), cx)))
			.child(unicode_emoji(glyph, rem(24.), cx))
			.into_any_element()
	}

	fn cells(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		let mut cells = Vec::new();
		if !self.query.is_empty() {
			let query = self.query.as_str();
			for (index, (name, id, animated)) in self.custom.iter().enumerate() {
				if name.to_lowercase().contains(query) {
					cells.push(self.custom_cell(index, name, *id, *animated, theme, cx));
				}
			}
			for (index, (name, glyph)) in emoji::all().enumerate() {
				if cells.len() >= SEARCH_LIMIT {
					break;
				}
				if name.contains(query) {
					cells.push(self.unicode_cell(index, glyph, theme, cx));
				}
			}
			return cells;
		}
		match self.tab {
			Tab::Custom => {
				for (index, (name, id, animated)) in self.custom.iter().enumerate() {
					cells.push(self.custom_cell(index, name, *id, *animated, theme, cx));
				}
			}
			Tab::Unicode(category) => {
				for (index, (_, glyph)) in CATEGORIES[category].emojis.iter().enumerate() {
					cells.push(self.unicode_cell(index, glyph, theme, cx));
				}
			}
		}
		cells
	}

	fn tab_button(
		&self,
		id: usize,
		glyph: &'static str,
		tab: Tab,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let active = self.tab == tab && self.query.is_empty();
		div()
			.id(("emoji-tab", id))
			.flex()
			.items_center()
			.justify_center()
			.size(rem(32.))
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(rem(18.))
			.when(active, |button| button.bg(theme.modifier_selected))
			.hover(|style| style.bg(theme.modifier_hover))
			.on_click(cx.listener(move |this, _, _, cx| {
				this.tab = tab;
				this.query.clear();
				cx.notify();
			}))
			.child(glyph)
			.into_any_element()
	}
}

impl Render for EmojiPicker {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let mut tabs: Vec<AnyElement> = Vec::new();
		if !self.custom.is_empty() {
			tabs.push(self.tab_button(usize::MAX, "\u{2B50}", Tab::Custom, &theme, cx));
		}
		for (index, category) in CATEGORIES.iter().enumerate() {
			tabs.push(self.tab_button(index, category.glyph, Tab::Unicode(index), &theme, cx));
		}
		let title: SharedString = if !self.query.is_empty() {
			cx.t("emoji-results")
		} else {
			match self.tab {
				Tab::Custom => cx.t("emoji-server"),
				Tab::Unicode(index) => cx.t(CATEGORIES[index].key),
			}
		};
		let cells = self.cells(&theme, cx);
		div()
			.flex()
			.flex_col()
			.w(rem(WIDTH))
			.h(rem(HEIGHT))
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.text_color(theme.text_normal)
			.child(
				div()
					.flex_none()
					.p(theme.space_8)
					.border_b_1()
					.border_color(theme.divider)
					.child(Input::new(&self.search).small()),
			)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_2)
					.px(theme.space_8)
					.py(theme.space_4)
					.children(tabs),
			)
			.child(
				div()
					.px(theme.space_12)
					.py(theme.space_4)
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_secondary)
					.child(title.to_uppercase()),
			)
			.child(
				div()
					.id("emoji-grid")
					.flex()
					.flex_wrap()
					.content_start()
					.flex_1()
					.min_h_0()
					.px(theme.space_8)
					.pb(theme.space_8)
					.children(cells)
					.overflow_y_scrollbar(),
			)
	}
}
