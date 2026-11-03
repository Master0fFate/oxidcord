use std::cmp::Reverse;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
	AnyElement, App, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render,
	SharedString, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Message, NotificationCenterItem, NotificationItemKind, Snowflake};
use session::Command;
use state::State;

use crate::components::{Avatar, IconButton, MentionBadge, PickHandler, segmented, select};
use crate::data::Data;
use crate::format;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::lookup::{Lookup, user_avatar};
use crate::shell::message::{Plain, Prefs, preview, snippet};
use crate::theme::Theme;

const SNIPPET_LENGTH: usize = 260;
const TITLE_LENGTH: usize = 160;
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
	ForYou,
	Unreads,
	Mentions,
}

const TABS: [(Tab, &str); 3] = [
	(Tab::ForYou, "inbox-tab-for-you"),
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

struct FeedRow {
	item: Arc<NotificationCenterItem>,
	title: SharedString,
	detail: Option<SharedString>,
	when: SharedString,
}

struct Feed {
	rows: Vec<FeedRow>,
	next: Option<Snowflake>,
}

fn kind_icon(kind: &NotificationItemKind) -> IconKind {
	match kind {
		NotificationItemKind::GoLivePush => IconKind::Video,
		NotificationItemKind::FriendRequestAccepted
		| NotificationItemKind::FriendRequestPending
		| NotificationItemKind::FriendRequestReminder
		| NotificationItemKind::FriendSuggestionCreated => IconKind::FriendRequests,
		NotificationItemKind::GameFriendRequestAccepted => IconKind::Gamepad,
		NotificationItemKind::DmFriendNudge | NotificationItemKind::MissedMessages => IconKind::Chat,
		NotificationItemKind::ScheduledGuildEventStarted => IconKind::Calendar,
		NotificationItemKind::ReactionSent => IconKind::Smile,
		NotificationItemKind::LifecycleItem => IconKind::Sparkles,
		_ => IconKind::Bell,
	}
}

fn feed_detail(item: &NotificationCenterItem, plain: &Plain) -> Option<String> {
	if let Some(content) = item
		.message_content
		.as_deref()
		.filter(|content| !content.is_empty())
	{
		return Some(snippet(&markdown::parse(content), SNIPPET_LENGTH, plain));
	}
	if let Some(message) = &item.message {
		return Some(preview(message, SNIPPET_LENGTH, plain));
	}
	item.callout.clone().filter(|callout| !callout.is_empty())
}

fn feed_rows(data: &Entity<Data>, cx: &App) -> Option<Feed> {
	let data = data.read(cx);
	let state = data.state();
	let items = state.notifications()?;
	let lookup = Lookup {
		state: &state,
		guild: None,
	};
	let plain = Plain {
		lookup: &lookup,
		h24: Prefs::load(cx).time_24h,
		cx,
	};
	let rows = items
		.iter()
		.map(|item| FeedRow {
			item: Arc::clone(item),
			title: snippet(&markdown::parse(&item.body), TITLE_LENGTH, &plain).into(),
			detail: feed_detail(item, &plain).map(Into::into),
			when: format::message_time(item.id.timestamp(), cx),
		})
		.collect();
	Some(Feed {
		rows,
		next: state.notifications_next(),
	})
}

pub struct InboxPanel {
	data: Entity<Data>,
	tab: Tab,
	filter: usize,
	unreads: Vec<UnreadRow>,
	feed: Option<Feed>,
}

impl EventEmitter<InboxEvent> for InboxPanel {}

impl InboxPanel {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let (_, roles, everyone) = MENTION_FILTERS[0];
		data.read(cx).send(Command::LoadMentions { roles, everyone });
		let unreads = unread_rows(&data.read(cx).state());
		let feed = feed_rows(&data, cx);
		Self {
			data,
			tab: Tab::Unreads,
			filter: 0,
			unreads,
			feed,
		}
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		self.unreads = unread_rows(&self.data.read(cx).state());
		if !self.feed_current(cx) {
			self.feed = feed_rows(&self.data, cx);
		}
		cx.notify();
	}

	fn feed_current(&self, cx: &Context<Self>) -> bool {
		let data = self.data.read(cx);
		let state = data.state();
		match (state.notifications(), &self.feed) {
			(None, None) => true,
			(Some(items), Some(feed)) => {
				items.len() == feed.rows.len()
					&& items
						.iter()
						.zip(&feed.rows)
						.all(|(item, row)| Arc::ptr_eq(item, &row.item))
					&& state.notifications_next() == feed.next
			}
			_ => false,
		}
	}

	fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
		self.tab = tab;
		if tab == Tab::ForYou {
			self.data
				.read(cx)
				.send(Command::LoadNotifications { after: None });
		}
		cx.notify();
	}

	fn mark_all_read(&mut self, cx: &mut Context<Self>) {
		if self.tab != Tab::ForYou {
			self.data.read(cx).send(Command::MarkAllRead);
			return;
		}
		let ids: Vec<Snowflake> = self
			.feed
			.iter()
			.flat_map(|feed| &feed.rows)
			.filter(|row| !row.item.acked)
			.map(|row| row.item.id)
			.collect();
		if !ids.is_empty() {
			self.data.read(cx).send(Command::AckNotifications { ids });
		}
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

	fn feed_card(&self, row: &FeedRow, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = row.item.id;
		let acked = row.item.acked;
		let jump = row.item.message_channel_id.zip(row.item.message_id);
		let lead = match &row.item.other_user {
			Some(user) => Avatar::new(user.global_name.as_deref().unwrap_or(&user.username).to_owned())
				.size(theme.portrait_md)
				.url(user_avatar(user, 64))
				.into_any_element(),
			None => div()
				.flex()
				.flex_none()
				.items_center()
				.justify_center()
				.size(theme.portrait_md)
				.rounded(theme.round(theme.portrait_md))
				.bg(theme.background_tertiary)
				.child(
					Icon::new(kind_icon(&row.item.kind))
						.size(theme.glyph_md)
						.color(theme.interactive_normal),
				)
				.into_any_element(),
		};
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
			.border_color(if acked { theme.divider } else { theme.brand })
			.child(lead)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.child(
						div()
							.text_size(theme.font_sm)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.line_clamp(2)
							.child(row.title.clone()),
					)
					.when_some(row.detail.clone(), |column, detail| {
						column.child(
							div()
								.text_size(theme.font_sm)
								.text_color(theme.text_normal)
								.line_clamp(3)
								.child(detail),
						)
					})
					.child(
						div()
							.text_size(theme.font_tiny)
							.text_color(theme.text_muted)
							.child(row.when.clone()),
					),
			)
			.when_some(jump, |card, (channel, message)| {
				card.child(
					Button::new(("inbox-feed-jump", id.get()))
						.small()
						.outline()
						.label(cx.t("pin-jump"))
						.on_click(cx.listener(move |this, _, _, cx| {
							if !acked {
								this.data
									.read(cx)
									.send(Command::AckNotifications { ids: vec![id] });
							}
							cx.emit(InboxEvent::Jump { channel, message })
						})),
				)
			})
			.when(!acked, |card| {
				card.child(
					IconButton::new(("inbox-feed-read", id.get()), IconKind::Check)
						.tooltip(cx.t("menu-mark-as-read"))
						.on_click(cx.listener(move |this, _, _, cx| {
							this.data
								.read(cx)
								.send(Command::AckNotifications { ids: vec![id] });
						})),
				)
			})
			.child(
				IconButton::new(("inbox-feed-dismiss", id.get()), IconKind::Close)
					.tooltip(cx.t("inbox-dismiss"))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.data.read(cx).send(Command::DismissNotification { id });
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
			Tab::ForYou => {
				let status = match &self.feed {
					None => Some("inbox-loading-feed"),
					Some(feed) if feed.rows.is_empty() => Some("inbox-empty-feed"),
					Some(_) => None,
				};
				let mut cards: Vec<AnyElement> = self
					.feed
					.iter()
					.flat_map(|feed| &feed.rows)
					.map(|row| self.feed_card(row, theme, cx))
					.collect();
				if let Some(next) = self.feed.as_ref().and_then(|feed| feed.next) {
					cards.push(
						div()
							.flex()
							.justify_center()
							.pb(theme.space_8)
							.child(
								Button::new("inbox-feed-more")
									.small()
									.outline()
									.label(cx.t("inbox-load-more"))
									.on_click(cx.listener(move |this, _, _, cx| {
										this.data
											.read(cx)
											.send(Command::LoadNotifications { after: Some(next) });
									})),
							)
							.into_any_element(),
					);
				}
				(cards, status)
			}
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
				let (_, roles, everyone) = MENTION_FILTERS[self.filter];
				let messages: Option<Vec<Arc<Message>>> = {
					let data = self.data.read(cx);
					let state = data.state();
					state.mentions(roles, everyone).map(|mentions| {
						mentions
							.iter()
							.filter(|message| !state.hides_messages_of(message.author.id))
							.cloned()
							.collect()
					})
				};
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
							.on_click(cx.listener(|this, _, _, cx| this.mark_all_read(cx))),
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
