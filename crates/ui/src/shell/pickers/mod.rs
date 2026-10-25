mod emoji;
mod gif;
mod sticker;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
	AppContext as _, Context, Entity, EventEmitter, Global, IntoElement, ParentElement as _, Render,
	Styled as _, Subscription, Window, div,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::Snowflake;
use session::Command;

use self::emoji::EmojiState;
use self::gif::GifState;
use self::sticker::StickerState;
use crate::components::{PickHandler, segmented};
use crate::data::Data;
use crate::i18n::Translate as _;
pub use crate::shell::emoji_picker::EmojiChoice;
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickerTab {
	Gif,
	Sticker,
	Emoji,
}

impl PickerTab {
	fn index(self) -> usize {
		match self {
			Self::Gif => 0,
			Self::Sticker => 1,
			Self::Emoji => 2,
		}
	}

	fn from_index(index: usize) -> Self {
		match index {
			0 => Self::Gif,
			1 => Self::Sticker,
			_ => Self::Emoji,
		}
	}

	fn placeholder(self) -> &'static str {
		match self {
			Self::Gif => "picker-search-gif",
			Self::Sticker => "picker-search-sticker",
			Self::Emoji => "picker-search-emoji",
		}
	}
}

pub struct SkinTone(pub Rc<Cell<usize>>);

impl Global for SkinTone {}

pub enum PickerEvent {
	Emoji(EmojiChoice),
	PluginPick {
		emoji: plugin_host::api::model::EmojiRef,
		channel: Snowflake,
	},
	Sent,
}

pub struct Picker {
	data: Entity<Data>,
	guild: Option<Snowflake>,
	channel: Snowflake,
	share_usage: bool,
	full: bool,
	plugin_pick: bool,
	tab: PickerTab,
	search: Entity<InputState>,
	query: String,
	tone: Rc<Cell<usize>>,
	gif: GifState,
	sticker: StickerState,
	emoji: EmojiState,
	_subscription: Subscription,
}

impl EventEmitter<PickerEvent> for Picker {}

impl Picker {
	pub fn new(
		data: Entity<Data>,
		(guild, channel): (Option<Snowflake>, Snowflake),
		tab: PickerTab,
		full: bool,
		share_usage: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let search = cx.new(|cx| InputState::new(window, cx));
		let subscription =
			cx.subscribe_in(
				&search,
				window,
				|this, input, event: &InputEvent, _, cx| match event {
					InputEvent::Change => {
						this.query = input.read(cx).value().trim().to_lowercase();
						this.on_query(cx);
					}
					InputEvent::PressEnter { .. } => this.on_submit(cx),
					_ => {}
				},
			);
		let tone = cx
			.try_global::<SkinTone>()
			.map_or_else(|| Rc::new(Cell::new(0)), |tone| tone.0.clone());
		let plugin_pick = data.read(cx).session().plugins().has_emoji_pick();
		let mut picker = Self {
			data,
			guild,
			channel,
			share_usage,
			full,
			plugin_pick,
			tab,
			search,
			query: String::new(),
			tone,
			gif: GifState::default(),
			sticker: StickerState::default(),
			emoji: EmojiState::default(),
			_subscription: subscription,
		};
		picker.enter_tab(tab, window, cx);
		picker
	}

	pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
		self.search.update(cx, |state, cx| state.focus(window, cx));
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		cx.notify();
	}

	fn enter_tab(&mut self, tab: PickerTab, window: &mut Window, cx: &mut Context<Self>) {
		self.tab = tab;
		self.query.clear();
		let placeholder = cx.t(tab.placeholder());
		self.search.update(cx, |state, cx| {
			state.set_value("", window, cx);
			state.set_placeholder(placeholder, window, cx);
			state.focus(window, cx);
		});
		match tab {
			PickerTab::Gif => {
				self.reset_gifs();
				self.load_gifs(cx);
			}
			PickerTab::Sticker => self.load_stickers(cx),
			PickerTab::Emoji => {}
		}
		cx.notify();
	}

	fn on_query(&mut self, cx: &mut Context<Self>) {
		if self.tab == PickerTab::Gif {
			self.gif_query_changed(cx);
		}
		cx.notify();
	}

	fn on_submit(&mut self, cx: &mut Context<Self>) {
		if self.tab == PickerTab::Gif {
			let query = self.query.clone();
			self.submit_gif_search(query, cx);
		}
	}

	fn set_query(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
		self.query.clone_from(&text);
		self.search
			.update(cx, |state, cx| state.set_value(text, window, cx));
	}

	fn send(&self, command: Command, cx: &Context<Self>) {
		self.data.read(cx).send(command);
	}

	fn tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
		let labels = [
			cx.t("picker-tab-gif"),
			cx.t("picker-tab-sticker"),
			cx.t("picker-tab-emoji"),
		];
		let weak = cx.weak_entity();
		let pick: PickHandler = Rc::new(move |index, window, cx| {
			weak.update(cx, |this, cx| {
				this.enter_tab(PickerTab::from_index(index), window, cx)
			})
			.ok();
		});
		segmented("picker-tabs", &labels, self.tab.index(), theme, pick)
	}
}

impl Render for Picker {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let body = match self.tab {
			PickerTab::Gif => self.gif_body(&theme, cx),
			PickerTab::Sticker => self.sticker_body(&theme, cx),
			PickerTab::Emoji => self.emoji_body(&theme, cx),
		};
		div()
			.flex()
			.flex_col()
			.w(theme.picker_width)
			.h(theme.picker_height)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.text_color(theme.text_normal)
			.overflow_hidden()
			.child(
				div()
					.flex()
					.flex_col()
					.flex_none()
					.gap(theme.space_8)
					.p(theme.space_12)
					.border_b_1()
					.border_color(theme.divider)
					.children(self.full.then(|| self.tabs(&theme, cx)))
					.child(Input::new(&self.search).small()),
			)
			.child(body)
	}
}
