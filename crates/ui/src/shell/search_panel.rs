use chrono::{NaiveDate, NaiveTime};
use gpui::{
	Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
	Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use rest::api::SearchQuery;
use state::State;

use super::message::snippet;
use crate::components::{Avatar, IconButton, badge};
use crate::data::Data;
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::lookup::user_avatar;
use crate::theme::{Theme, rem};

const SNIPPET_LENGTH: usize = 220;
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;
const SNOWFLAKE_TIME_SHIFT: u32 = 22;
const HAS_VALUES: [&str; 7] = ["image", "video", "sound", "file", "sticker", "embed", "link"];

pub enum SearchEvent {
	Jump { channel: Snowflake, message: Snowflake },
	Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
	From,
	Mentions,
	Has,
	Before,
	After,
}

impl Filter {
	fn key(self) -> &'static str {
		match self {
			Self::From => "from",
			Self::Mentions => "mentions",
			Self::Has => "has",
			Self::Before => "before",
			Self::After => "after",
		}
	}

	fn named(key: &str) -> Option<Self> {
		[Self::From, Self::Mentions, Self::Has, Self::Before, Self::After]
			.into_iter()
			.find(|filter| filter.key().eq_ignore_ascii_case(key))
	}
}

struct Chip {
	filter: Filter,
	value: String,
}

fn snowflake_at(date: NaiveDate, end_of_day: bool) -> Option<Snowflake> {
	let day = if end_of_day { date.succ_opt()? } else { date };
	let millis = day.and_time(NaiveTime::MIN).and_utc().timestamp_millis() - DISCORD_EPOCH_MS;
	u64::try_from(millis)
		.ok()
		.map(|millis| Snowflake::from(millis << SNOWFLAKE_TIME_SHIFT))
}

fn resolve_user(
	state: &State,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
	name: &str,
) -> Option<Snowflake> {
	let cleaned = name.trim_start_matches('@');
	let digits = cleaned
		.trim_start_matches("<@")
		.trim_start_matches('!')
		.trim_end_matches('>');
	if let Ok(id) = digits.parse::<u64>() {
		return Some(Snowflake::from(id));
	}
	let needle = cleaned.to_lowercase();
	let candidates: Vec<Snowflake> = match guild.and_then(|id| state.guild(id)) {
		Some(found) => found.members.keys().copied().collect(),
		None => state
			.dm_rows()
			.into_iter()
			.filter(|row| Some(row.channel_id) == channel)
			.flat_map(|row| row.recipients)
			.chain(state.current_user_id())
			.collect(),
	};
	let labels = |id: Snowflake| {
		[
			state.display_name(guild, id).to_lowercase(),
			state
				.user(id)
				.map(|user| user.username.to_lowercase())
				.unwrap_or_default(),
		]
	};
	candidates
		.iter()
		.copied()
		.find(|id| labels(*id).contains(&needle))
		.or_else(|| {
			candidates
				.iter()
				.copied()
				.find(|id| labels(*id).iter().any(|label| label.starts_with(&needle)))
		})
}

fn parse_query(
	text: &str,
	state: &State,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
) -> (SearchQuery, Vec<Chip>) {
	let mut query = SearchQuery::default();
	let mut chips = Vec::new();
	let mut content: Vec<&str> = Vec::new();
	for token in text.split_whitespace() {
		let applied = token.split_once(':').and_then(|(key, value)| {
			let filter = Filter::named(key)?;
			(!value.is_empty()).then_some((filter, value))
		});
		let accepted = applied.is_some_and(|(filter, value)| match filter {
			Filter::From => resolve_user(state, guild, channel, value)
				.map(|id| query.author_id = Some(id))
				.is_some(),
			Filter::Mentions => resolve_user(state, guild, channel, value)
				.map(|id| query.mentions = Some(id))
				.is_some(),
			Filter::Has => {
				let lowered = value.to_lowercase();
				HAS_VALUES.contains(&lowered.as_str()) && {
					query.has.push(lowered);
					true
				}
			}
			Filter::Before => NaiveDate::parse_from_str(value, "%Y-%m-%d")
				.ok()
				.and_then(|date| snowflake_at(date, false))
				.map(|id| query.before = Some(id))
				.is_some(),
			Filter::After => NaiveDate::parse_from_str(value, "%Y-%m-%d")
				.ok()
				.and_then(|date| snowflake_at(date, true))
				.map(|id| query.after = Some(id))
				.is_some(),
		});
		match applied {
			Some((filter, value)) if accepted => chips.push(Chip {
				filter,
				value: value.to_owned(),
			}),
			_ => content.push(token),
		}
	}
	if !content.is_empty() {
		query.content = Some(content.join(" "));
	}
	(query, chips)
}

struct Hit {
	channel: Snowflake,
	message: Snowflake,
	author: SharedString,
	avatar: String,
	time: SharedString,
	place: SharedString,
	text: SharedString,
}

pub struct SearchPanel {
	data: Entity<Data>,
	chips: Vec<Chip>,
	hits: Vec<Hit>,
	total: u64,
	searching: bool,
}

impl EventEmitter<SearchEvent> for SearchPanel {}

impl SearchPanel {
	pub fn new(data: Entity<Data>) -> Self {
		Self {
			data,
			chips: Vec::new(),
			hits: Vec::new(),
			total: 0,
			searching: false,
		}
	}

	pub fn begin(
		&mut self,
		text: &str,
		guild: Option<Snowflake>,
		channel: Option<Snowflake>,
		cx: &mut Context<Self>,
	) -> SearchQuery {
		let (query, chips) = parse_query(text, &self.data.read(cx).state(), guild, channel);
		self.chips = chips;
		self.searching = true;
		self.hits.clear();
		self.total = 0;
		cx.notify();
		query
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let Some(results) = data.search_results() else {
			return;
		};
		let state = data.state();
		self.total = results.total;
		self.hits = results
			.messages
			.iter()
			.map(|message| {
				let guild = message
					.guild_id
					.or_else(|| state.channel_guild(message.channel_id));
				let channel = state.channel(message.channel_id);
				let place = channel
					.and_then(|channel| channel.name.clone())
					.map_or_else(String::new, |name| format!("#{name}"));
				Hit {
					channel: message.channel_id,
					message: message.id,
					author: state.display_name(guild, message.author.id).into(),
					avatar: user_avatar(&message.author, 64),
					time: format::message_time(
						message.timestamp.unwrap_or_else(|| message.id.timestamp()),
						cx,
					),
					place: place.into(),
					text: snippet(&markdown::parse(&message.content), SNIPPET_LENGTH).into(),
				}
			})
			.collect();
		self.searching = false;
		drop(state);
		cx.notify();
	}
}

impl Render for SearchPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let title = if self.searching {
			cx.t("search-searching")
		} else {
			cx.t_args(
				"search-results",
				&[("count", ArgValue::from(self.total as usize))],
			)
		};
		let rows = self.hits.iter().enumerate().map(|(index, hit)| {
			let channel = hit.channel;
			let message = hit.message;
			div()
				.id(("search-hit", index))
				.flex()
				.flex_col()
				.gap(theme.space_4)
				.mx(theme.space_8)
				.mb(theme.space_8)
				.p(theme.space_8)
				.rounded(theme.radius_md)
				.bg(theme.background_primary)
				.cursor_pointer()
				.hover(|style| style.bg(theme.modifier_selected))
				.on_click(cx.listener(move |_, _, _, cx| cx.emit(SearchEvent::Jump { channel, message })))
				.child(
					div()
						.flex()
						.items_center()
						.gap(theme.space_8)
						.child(
							Avatar::new(hit.author.clone())
								.size(rem(24.))
								.url(hit.avatar.clone()),
						)
						.child(
							div()
								.text_size(theme.font_sm)
								.font_weight(FontWeight::SEMIBOLD)
								.text_color(theme.header_primary)
								.child(hit.author.clone()),
						)
						.child(
							div()
								.text_size(theme.font_tiny)
								.text_color(theme.text_muted)
								.child(hit.time.clone()),
						),
				)
				.child(
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_normal)
						.line_clamp(4)
						.child(hit.text.clone()),
				)
				.when(!hit.place.is_empty(), |card| {
					card.child(
						div()
							.text_size(theme.font_tiny)
							.text_color(theme.text_muted)
							.child(hit.place.clone()),
					)
				})
		});
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(rem(340.))
			.h_full()
			.bg(theme.background_secondary)
			.border_l_1()
			.border_color(theme.background_tertiary)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.justify_between()
					.h(theme.top_bar_height)
					.px(theme.space_16)
					.border_b_1()
					.border_color(theme.background_tertiary)
					.child(
						div()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(title),
					)
					.child(
						IconButton::new("search-close", IconKind::Close)
							.tooltip(cx.t("search-close"))
							.on_click(cx.listener(|_, _, _, cx| cx.emit(SearchEvent::Close))),
					),
			)
			.when(!self.chips.is_empty(), |panel| {
				panel.child(
					div()
						.flex()
						.flex_none()
						.flex_wrap()
						.gap(theme.space_4)
						.px(theme.space_16)
						.pt(theme.space_8)
						.children(self.chips.iter().map(|chip| {
							badge(
								format!("{}: {}", chip.filter.key(), chip.value),
								theme.brand,
								&theme,
							)
						})),
				)
			})
			.child(
				div()
					.flex()
					.flex_col()
					.pt(theme.space_8)
					.children(rows)
					.overflow_y_scrollbar()
					.id("search-results")
					.flex_1()
					.min_h_0(),
			)
	}
}
