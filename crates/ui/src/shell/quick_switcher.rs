use gpui::{
	AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;

use crate::components::Avatar;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

const MAX_RESULTS: usize = 9;
const MAX_RECENT: usize = 5;
const MATCH: i32 = 10;
const ADJACENT_BONUS: i32 = 15;
const WORD_START_BONUS: i32 = 20;
const LEADING_GAP_PENALTY: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitcherTarget {
	Channel {
		guild: Option<Snowflake>,
		channel: Snowflake,
	},
	Guild(Snowflake),
	Friend(Snowflake),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
	Recent,
	Channels,
	Voice,
	DirectMessages,
	Friends,
	Servers,
}

impl Category {
	fn rank(self) -> u8 {
		match self {
			Self::Recent => 0,
			Self::Channels => 1,
			Self::Voice => 2,
			Self::DirectMessages => 3,
			Self::Friends => 4,
			Self::Servers => 5,
		}
	}

	fn key(self) -> &'static str {
		match self {
			Self::Recent => "switcher-category-recent",
			Self::Channels => "switcher-category-channels",
			Self::Voice => "switcher-category-voice",
			Self::DirectMessages => "switcher-category-dms",
			Self::Friends => "switcher-category-friends",
			Self::Servers => "switcher-category-servers",
		}
	}

	fn narrowed_by(self, prefix: char) -> bool {
		match prefix {
			'#' => self == Self::Channels,
			'!' => self == Self::Voice,
			'@' => matches!(self, Self::DirectMessages | Self::Friends),
			'*' => self == Self::Servers,
			_ => true,
		}
	}
}

#[derive(Clone)]
pub enum Leading {
	Icon(IconKind),
	Avatar(String),
}

#[derive(Clone)]
pub struct SwitcherEntry {
	pub target: SwitcherTarget,
	pub name: SharedString,
	pub context: Option<SharedString>,
	pub leading: Leading,
	pub category: Category,
}

pub enum QuickSwitcherEvent {
	Pick(SwitcherTarget),
}

struct Shown {
	index: usize,
	category: Category,
}

pub struct QuickSwitcher {
	input: Entity<InputState>,
	entries: Vec<SwitcherEntry>,
	recent: Vec<SwitcherTarget>,
	query: String,
	selected: usize,
}

impl EventEmitter<QuickSwitcherEvent> for QuickSwitcher {}

pub(super) fn fuzzy_score(query: &[char], name: &str) -> Option<i32> {
	let mut matched = 0;
	let mut score = 0;
	let mut previous: Option<usize> = None;
	let mut previous_char = ' ';
	for (at, character) in name.chars().enumerate() {
		if matched < query.len() && character.to_lowercase().eq(query[matched].to_lowercase()) {
			score += MATCH;
			if previous.is_some_and(|last| last + 1 == at) {
				score += ADJACENT_BONUS;
			}
			if !previous_char.is_alphanumeric() {
				score += WORD_START_BONUS;
			}
			if previous.is_none() {
				score -= at as i32 * LEADING_GAP_PENALTY;
			}
			previous = Some(at);
			matched += 1;
		}
		previous_char = character;
	}
	(matched == query.len()).then_some(score)
}

impl QuickSwitcher {
	pub fn new(
		entries: Vec<SwitcherEntry>,
		recent: Vec<SwitcherTarget>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let placeholder = cx.t("quick-switcher-placeholder");
		let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		cx.subscribe_in(
			&input,
			window,
			|this, input, event: &InputEvent, _, cx| match event {
				InputEvent::Change => {
					this.query = input.read(cx).value().to_string();
					this.selected = 0;
					cx.notify();
				}
				InputEvent::PressEnter { .. } => this.pick(cx),
				_ => {}
			},
		)
		.detach();
		Self {
			input,
			entries,
			recent,
			query: String::new(),
			selected: 0,
		}
	}

	pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
		self.input.update(cx, |state, cx| state.focus(window, cx));
	}

	fn recency(&self, target: SwitcherTarget) -> usize {
		self.recent
			.iter()
			.position(|recent| *recent == target)
			.unwrap_or(usize::MAX)
	}

	fn results(&self) -> Vec<Shown> {
		let trimmed = self.query.trim_start();
		let prefix = trimmed
			.chars()
			.next()
			.filter(|c| matches!(c, '#' | '!' | '@' | '*'));
		let text = prefix
			.map_or(trimmed, |prefix| &trimmed[prefix.len_utf8()..])
			.trim();
		if text.is_empty() && prefix.is_none() {
			return self.recent_results();
		}
		let needle: Vec<char> = text.chars().collect();
		let mut scored: Vec<(i32, usize, usize)> = self
			.entries
			.iter()
			.enumerate()
			.filter(|(_, entry)| prefix.is_none_or(|prefix| entry.category.narrowed_by(prefix)))
			.filter_map(|(index, entry)| {
				let score = if needle.is_empty() {
					0
				} else {
					fuzzy_score(&needle, &entry.name)?
				};
				Some((score, self.recency(entry.target), index))
			})
			.collect();
		scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
		scored.truncate(MAX_RESULTS);
		let mut shown: Vec<Shown> = scored
			.into_iter()
			.map(|(_, _, index)| Shown {
				index,
				category: self.entries[index].category,
			})
			.collect();
		shown.sort_by_key(|item| item.category.rank());
		shown
	}

	fn recent_results(&self) -> Vec<Shown> {
		let mut shown: Vec<Shown> = self
			.recent
			.iter()
			.filter_map(|target| self.entries.iter().position(|entry| entry.target == *target))
			.take(MAX_RECENT)
			.map(|index| Shown {
				index,
				category: Category::Recent,
			})
			.collect();
		let taken: Vec<usize> = shown.iter().map(|item| item.index).collect();
		let mut rest: Vec<Shown> = self
			.entries
			.iter()
			.enumerate()
			.filter(|(index, _)| !taken.contains(index))
			.take(MAX_RESULTS - shown.len())
			.map(|(index, entry)| Shown {
				index,
				category: entry.category,
			})
			.collect();
		rest.sort_by_key(|item| item.category.rank());
		shown.extend(rest);
		shown
	}

	fn pick(&mut self, cx: &mut Context<Self>) {
		let picked = self
			.results()
			.get(self.selected)
			.map(|item| self.entries[item.index].target);
		if let Some(target) = picked {
			cx.emit(QuickSwitcherEvent::Pick(target));
		}
	}

	fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
		let count = self.results().len();
		if count == 0 {
			return;
		}
		self.selected = if forward {
			(self.selected + 1) % count
		} else {
			(self.selected + count - 1) % count
		};
		cx.notify();
	}
}

impl Render for QuickSwitcher {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let shown = self.results();
		let mut children = Vec::new();
		let mut previous: Option<Category> = None;
		for (position, item) in shown.iter().enumerate() {
			let entry = &self.entries[item.index];
			if previous != Some(item.category) {
				previous = Some(item.category);
				children.push(
					div()
						.px(theme.space_8)
						.pt(theme.space_8)
						.pb(theme.space_4)
						.text_size(theme.font_tiny)
						.font_weight(FontWeight::BOLD)
						.text_color(theme.header_secondary)
						.child(cx.t(item.category.key()).to_uppercase())
						.into_any_element(),
				);
			}
			let selected = position == self.selected;
			let target = entry.target;
			let leading = match &entry.leading {
				Leading::Icon(kind) => Icon::new(*kind)
					.size(theme.icon_md)
					.color(theme.text_muted)
					.into_any_element(),
				Leading::Avatar(url) => Avatar::new(entry.name.clone())
					.size(theme.icon_lg)
					.url(url.clone())
					.cutout(theme.background_floating)
					.into_any_element(),
			};
			children.push(
				div()
					.id(("switcher-result", position))
					.flex()
					.items_center()
					.gap(theme.space_8)
					.h(theme.field_height)
					.px(theme.space_8)
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.text_color(if selected {
						theme.interactive_active
					} else {
						theme.interactive_normal
					})
					.when(selected, |row| row.bg(theme.modifier_selected))
					.when(!selected, |row| row.hover(|style| style.bg(theme.modifier_hover)))
					.on_click(cx.listener(move |_, _, _, cx| cx.emit(QuickSwitcherEvent::Pick(target))))
					.child(
						div()
							.flex()
							.flex_none()
							.justify_center()
							.w(theme.icon_lg)
							.child(leading),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.child(entry.name.clone()),
					)
					.children(entry.context.clone().map(|context| {
						div()
							.flex_none()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(context)
					}))
					.into_any_element(),
			);
		}
		let empty = shown.is_empty();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.p(theme.space_16)
			.on_action(cx.listener(|this, _: &MoveDown, _, cx| this.step(true, cx)))
			.on_action(cx.listener(|this, _: &MoveUp, _, cx| this.step(false, cx)))
			.child(Input::new(&self.input).large())
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_2)
					.max_h(theme.list_max_height + theme.avatar_lg)
					.children(children)
					.when(empty, |list| {
						list.child(
							div()
								.p(theme.space_8)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(cx.t("quick-switcher-empty")),
						)
					})
					.overflow_y_scrollbar()
					.id("switcher-list"),
			)
			.child(
				div()
					.pt(theme.space_8)
					.border_t_1()
					.border_color(theme.divider)
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.text_muted)
					.child(cx.t("quick-switcher-tip")),
			)
	}
}
