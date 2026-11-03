use std::cmp::Reverse;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
	AnyElement, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render,
	SharedString, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Message, Snowflake};
use session::Command;
use state::State;

use crate::components::{Avatar, IconButton, MentionBadge, PickHandler, segmented, select};
use crate::data::Data;
use crate::format;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::lookup::{Lookup, user_avatar};
use crate::shell::message::{Plain, Prefs, preview};
use crate::theme::Theme;

const SNIPPET_LENGTH: usize = 260;
const MENTION_FILTERS: [(&str, bool, bool); 4] = [
	("inbox-filter-all", true, true),
	("inbox-filter-no-roles", false, true),
	("inbox-filter-no-everyone", true, false),
	("inbox-filter-direct", false, false),
];

pub enum InboxEvent {
	Jump { channel: Snowflake, message: Snowflake },
	Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
	Unreads,
	Mentions,
}

const TABS: [(Tab, &str); 2] = [
	(Tab::Unreads, "inbox-tab-unreads"),
	(Tab::Mentions, "inbox-tab-mentions"),
];

struct UnreadRow {
	channel: Snowflake,
	jump: Snowflake,
	guild: Option<String>,
	name: String,
	mentions: u32,
}

fn unread_rows(state: &State) -> Vec<UnreadRow> {
	let mut entries = state.unread_channels(None, true);
	entries.retain(|entry| !entry.muted);
	entries.sort_unstable_by_key(|entry| Reverse(entry.last_message_id));
	entries
		.into_iter()
		.filter_map(|entry| {
			let jump = entry.last_read_id.or(entry.last_message_id)?;
			let (guild, name) = match state.channel_guild(entry.channel_id) {
				Some(guild_id) => (
					state.guild(guild_id).map(|found| found.name.clone()),
					state
						.channel(entry.channel_id)
						.and_then(|channel| channel.name.as_deref())
						.map(|name| format!("#{name}"))
						.unwrap_or_default(),
				),
				None => (
					None,
					state
						.dm_row(entry.channel_id)
						.map(|row| row.name)
						.unwrap_or_default(),
				),
			};
			Some(UnreadRow {
				channel: entry.channel_id,
				jump,
				guild,
				name,
				mentions: entry.mentions,
			})
		})
		.collect()
}

pub struct InboxPanel {
	data: Entity<Data>,
	tab: Tab,
	filter: usize,
	unreads: Vec<UnreadRow>,
}

impl EventEmitter<InboxEvent> for InboxPanel {}

impl InboxPanel {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let (_, roles, everyone) = MENTION_FILTERS[0];
		data.read(cx).send(Command::LoadMentions { roles, everyone });
		let unreads = unread_rows(&data.read(cx).state());
		Self {
			data,
			tab: Tab::Unreads,
			filter: 0,
			unreads,
		}
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		self.unreads = unread_rows(&self.data.read(cx).state());
		cx.notify();
	}

	fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
		self.tab = tab;
		cx.notify();
	}

	fn set_filter(&mut self, filter: usize, cx: &mut Context<Self>) {
		self.filter = filter;
		let (_, roles, everyone) = MENTION_FILTERS[filter];
		self.data.read(cx).send(Command::LoadMentions { roles, everyone });
		cx.notify();
	}

	fn tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let labels: Vec<SharedString> = TABS.iter().map(|(_, key)| cx.t(key)).collect();
		let selected = TABS
			.iter()
			.position(|(tab, _)| *tab == self.tab)
			.unwrap_or_default();
		let panel = cx.entity();
		let pick_tab: PickHandler = Rc::new({
			let panel = panel.clone();
			move |index, _, cx| panel.update(cx, |this, cx| this.set_tab(TABS[index].0, cx))
		});
		let filters: Vec<SharedString> = MENTION_FILTERS.iter().map(|(key, ..)| cx.t(key)).collect();
		let pick_filter: PickHandler =
			Rc::new(move |index, _, cx| panel.update(cx, |this, cx| this.set_filter(index, cx)));
		div()
			.flex()
			.flex_none()
			.items_center()
			.justify_between()
			.gap(theme.space_8)
			.px(theme.space_16)
			.py(theme.space_8)
			.child(segmented("inbox-tab", &labels, selected, theme, pick_tab))
			.when(self.tab == Tab::Mentions, |bar| {
				bar.child(select(
					"inbox-filter",
					filters[self.filter].clone(),
					&filters,
					self.filter,
					pick_filter,
				))
			})
			.into_any_element()
	}

	fn unread_card(&self, row: &UnreadRow, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let channel = row.channel;
		let jump = row.jump;
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.mx(theme.space_8)
			.mb(theme.space_8)
			.p(theme.space_12)
			.rounded(theme.radius_md)
			.bg(theme.background_primary)
			.border_1()
			.border_color(theme.divider)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.when_some(row.guild.clone(), |column, guild| {
						column.child(
							div()
								.truncate()
								.text_size(theme.font_tiny)
								.font_weight(FontWeight::SEMIBOLD)
								.text_color(theme.text_muted)
								.child(guild),
						)
					})
					.child(
						div()
							.truncate()
							.text_size(theme.font_sm)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(row.name.clone()),
					),
			)
			.when(row.mentions > 0, |card| {
				card.child(MentionBadge::new(row.mentions))
			})
			.child(
				Button::new(("inbox-unread-jump", channel.get()))
					.small()
					.outline()
					.label(cx.t("pin-jump"))
					.on_click(cx.listener(move |_, _, _, cx| {
						cx.emit(InboxEvent::Jump {
							channel,
							message: jump,
						})
					})),
			)
			.child(
				IconButton::new(("inbox-unread-read", channel.get()), IconKind::Check)
					.tooltip(cx.t("menu-mark-as-read"))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.data
							.read(cx)
							.send(Command::MarkChannelRead { channel_id: channel });
					})),
			)
			.into_any_element()
	}

	fn card(&self, message: &Arc<Message>, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = message.id;
		let channel = message.channel_id;
		let (name, place, text) = {
			let data = self.data.read(cx);
			let state = data.state();
			let guild = state.channel_guild(channel);
			let place = match (state.channel(channel).and_then(|found| found.name.clone()), guild) {
				(Some(channel_name), Some(guild_id)) => match state.guild(guild_id) {
					Some(found) => format!("{} \u{b7} #{channel_name}", found.name),
					None => format!("#{channel_name}"),
				},
				(Some(channel_name), None) => channel_name,
				(None, _) => String::new(),
			};
			let lookup = Lookup { state: &state, guild };
			let plain = Plain {
				lookup: &lookup,
				h24: Prefs::load(cx).time_24h,
				cx,
			};
			let text = preview(message, SNIPPET_LENGTH, &plain);
			(state.display_name(guild, message.author.id), place, text)
		};
		let when = format::message_time(message.timestamp.unwrap_or_else(|| id.timestamp()), cx);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.mx(theme.space_8)
			.mb(theme.space_8)
			.p(theme.space_12)
			.rounded(theme.radius_md)
			.bg(theme.background_primary)
			.border_1()
			.border_color(theme.divider)
			.child(
				div()
					.truncate()
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.text_muted)
					.child(place),
			)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						Avatar::new(name.clone())
							.size(theme.portrait_md)
							.url(user_avatar(&message.author, 64)),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.flex_1()
							.min_w_0()
							.child(
								div()
									.truncate()
									.text_size(theme.font_sm)
									.font_weight(FontWeight::SEMIBOLD)
									.text_color(theme.header_primary)
									.child(name),
							)
							.child(
								div()
									.text_size(theme.font_tiny)
									.text_color(theme.text_muted)
									.child(when),
							),
					)
					.child(
						Button::new(("inbox-jump", id.get()))
							.small()
							.outline()
							.label(cx.t("pin-jump"))
							.on_click(cx.listener(move |_, _, _, cx| {
								cx.emit(InboxEvent::Jump { channel, message: id })
							})),
					)
					.child(
						IconButton::new(("inbox-dismiss", id.get()), IconKind::Close)
							.tooltip(cx.t("inbox-dismiss"))
							.on_click(cx.listener(move |this, _, _, cx| {
								this.data
									.read(cx)
									.send(Command::DismissMention { message_id: id });
							})),
					),
			)
			.child(
				div()
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.line_clamp(5)
					.child(text),
			)
			.into_any_element()
	}

	fn rows(&self, theme: &Theme, cx: &mut Context<Self>) -> (Vec<AnyElement>, Option<&'static str>) {
		match self.tab {
			Tab::Unreads => {
				let status = self.unreads.is_empty().then_some("inbox-empty-unreads");
				let cards = self
					.unreads
					.iter()
					.map(|row| self.unread_card(row, theme, cx))
					.collect();
				(cards, status)
			}
			Tab::Mentions => {
				let messages: Option<Vec<Arc<Message>>> = self
					.data
					.read(cx)
					.state()
					.mentions()
					.map(|mentions| mentions.to_vec());
				let status = match &messages {
					None => Some("inbox-loading"),
					Some(found) if found.is_empty() => Some("inbox-empty"),
					Some(_) => None,
				};
				let cards = messages
					.iter()
					.flatten()
					.map(|message| self.card(message, theme, cx))
					.collect();
				(cards, status)
			}
		}
	}
}

impl Render for InboxPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (cards, status) = self.rows(&theme, cx);
		div()
			.flex()
			.flex_col()
			.w(theme.popover_width)
			.h(theme.popover_height)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_secondary)
			.text_color(theme.text_normal)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_8)
					.h(theme.top_bar_height)
					.px(theme.space_16)
					.border_b_1()
					.border_color(theme.divider)
					.child(
						Icon::new(IconKind::Inbox)
							.size(theme.glyph_lg)
							.color(theme.text_muted),
					)
					.child(
						div()
							.flex_1()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(cx.t("inbox-title")),
					)
					.child(
						IconButton::new("inbox-mark-all", IconKind::Check)
							.tooltip(cx.t("inbox-mark-all-read"))
							.on_click(cx.listener(|this, _, _, cx| {
								this.data.read(cx).send(Command::MarkAllRead);
							})),
					)
					.child(
						IconButton::new("inbox-close", IconKind::Close)
							.tooltip(cx.t("action-close"))
							.on_click(cx.listener(|_, _, _, cx| cx.emit(InboxEvent::Close))),
					),
			)
			.child(self.tabs(&theme, cx))
			.child(
				div()
					.flex()
					.flex_col()
					.children(cards)
					.when_some(status, |list, label| {
						list.child(
							div()
								.flex()
								.flex_col()
								.items_center()
								.gap(theme.space_8)
								.p(theme.space_32)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(
									Icon::new(IconKind::Inbox)
										.size(theme.portrait_lg)
										.color(theme.interactive_muted),
								)
								.child(cx.t(label)),
						)
					})
					.overflow_y_scrollbar()
					.id("inbox-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
