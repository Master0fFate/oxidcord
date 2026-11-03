use std::rc::Rc;
use std::time::Duration;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, Hsla, InteractiveElement as _,
	IntoElement, ListAlignment, ListState, ParentElement as _, Pixels, Point, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, list,
	prelude::FluentBuilder as _, px,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::scroll::ScrollableElement as _;
use plugin_host::ViewContext;
use protocol::Snowflake;
use session::{Command, MEMBER_LIST_WINDOW};
use state::{MemberListRow, State, UserOverride};

use super::account::{Relation, manage_items, primary_items, relation_facts};
use super::community::developer_mode;
use super::moderation;
use super::plugins::{self, MenuTarget, PluginMenu as _};
use crate::components::{Avatar, ContextMenuBuilder, IconButton};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::model::{Status, role_display_color, status_of};
use crate::theme::{Theme, rem};

const AVATAR_PIXELS: u32 = 64;
const SEARCH_LIMIT: usize = 100;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(350);
const SEARCH_GROUP: &str = "search";
const SEARCH_HEIGHT: f32 = 30.;
const RANGE_DEBOUNCE: Duration = Duration::from_millis(150);
const ROW_OVERDRAW: f32 = 200.;
const PLACEHOLDER_NAME_WIDTH: f32 = 96.;

pub enum MemberListEvent {
	OpenProfile { user: Snowflake, at: Point<Pixels> },
}

#[derive(PartialEq)]
struct MemberItem {
	id: Snowflake,
	name: SharedString,
	color: Option<Hsla>,
	status: Status,
	activity: Option<SharedString>,
	avatar: String,
	bot: bool,
	badge: Option<SharedString>,
	over: Option<UserOverride>,
}

#[derive(PartialEq)]
enum Row {
	Group { id: String, name: String, count: u32 },
	Member(Box<MemberItem>),
}

#[derive(Clone, Copy, PartialEq)]
struct Windows {
	channel: Snowflake,
	first: u32,
	last: u32,
}

impl Windows {
	fn top(channel: Snowflake) -> Self {
		Self {
			channel,
			first: 0,
			last: 0,
		}
	}

	fn rows(self) -> std::ops::Range<u32> {
		self.first * MEMBER_LIST_WINDOW..(self.last + 1) * MEMBER_LIST_WINDOW
	}
}

fn selected_channel(state: &State, guild: Snowflake) -> Option<Snowflake> {
	state
		.selected()
		.filter(|selected| selected.guild_id == Some(guild))
		.map(|selected| selected.channel_id)
}

pub struct MemberList {
	data: Entity<Data>,
	guild: Option<Snowflake>,
	rows: Vec<(usize, Row)>,
	total: usize,
	list_id: String,
	scroll: ListState,
	subscribed: Option<Windows>,
	queued: Option<Windows>,
	range_task: Option<Task<()>>,
	query: String,
	input: Option<Entity<InputState>>,
	clear_input: bool,
	remote_search: Option<Task<()>>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<MemberListEvent> for MemberList {}

impl MemberList {
	pub fn new(data: Entity<Data>) -> Self {
		Self {
			data,
			guild: None,
			rows: Vec::new(),
			total: 0,
			list_id: String::new(),
			scroll: ListState::new(0, ListAlignment::Top, px(ROW_OVERDRAW)),
			subscribed: None,
			queued: None,
			range_task: None,
			query: String::new(),
			input: None,
			clear_input: false,
			remote_search: None,
			_subscriptions: Vec::new(),
		}
	}

	pub fn show(&mut self, guild: Option<Snowflake>, cx: &mut Context<Self>) {
		if self.guild != guild && !self.query.is_empty() {
			self.query.clear();
			self.clear_input = true;
			self.remote_search = None;
		}
		if self.guild != guild {
			self.list_id.clear();
		}
		self.guild = guild;
		self.refresh(cx);
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let Some(guild) = self.guild else {
			return;
		};
		let data = self.data.read(cx);
		let state = data.state();
		let searching = !self.query.is_empty();
		let (rows, total) = if searching {
			let rows: Vec<(usize, Row)> = search_rows(&state, guild, &self.query)
				.into_iter()
				.enumerate()
				.collect();
			let total = rows.len();
			(rows, total)
		} else {
			let rows = state
				.member_rows_at(guild)
				.into_iter()
				.map(|(index, row)| {
					let row = match row {
						MemberListRow::Group { id, name, count } => Row::Group { id, name, count },
						MemberListRow::Member(member) => Row::Member(Box::new(member_item(
							&state,
							(guild, member.user_id),
							member.display,
							member.color,
							member.badge,
							Status::from(&member.status),
							member.activity,
						))),
					};
					(index, row)
				})
				.collect();
			(rows, state.member_row_count(guild))
		};
		let list_id = if searching {
			SEARCH_GROUP
		} else {
			state
				.guild(guild)
				.map_or("", |found| found.member_list.id.as_str())
		};
		let reset = self.list_id != list_id;
		if reset {
			self.list_id = list_id.to_owned();
		}
		let selected = selected_channel(&state, guild);
		drop(state);
		let channel_changed = self
			.subscribed
			.is_some_and(|windows| Some(windows.channel) != selected);
		if channel_changed {
			self.subscribed = None;
		}
		if reset {
			self.scroll.reset(total);
		} else if total > self.total {
			self.scroll.splice(self.total..self.total, total - self.total);
		} else if total < self.total {
			self.scroll.splice(total..self.total, 0);
		}
		if !reset && !same_shape(&rows, &self.rows) {
			self.scroll.remeasure();
		}
		if reset || total != self.total || rows != self.rows {
			self.rows = rows;
			self.total = total;
			cx.notify();
		} else if channel_changed {
			cx.notify();
		}
	}

	fn track_visible(&mut self, theme: &Theme, cx: &mut Context<Self>) {
		let Some(guild) = self.guild.filter(|_| self.query.is_empty()) else {
			return;
		};
		let Some(channel) = selected_channel(&self.data.read(cx).state(), guild) else {
			return;
		};
		let top = self.scroll.logical_scroll_top().item_ix as u32;
		let height = self.scroll.viewport_bounds().size.height;
		let visible = (height / theme.pixels(theme.member_row_height)).ceil() as u32 + 1;
		let wanted = Windows {
			channel,
			first: top / MEMBER_LIST_WINDOW,
			last: (top + visible) / MEMBER_LIST_WINDOW,
		};
		let subscribed = self
			.subscribed
			.filter(|windows| windows.channel == channel)
			.unwrap_or(Windows::top(channel));
		self.subscribed = Some(subscribed);
		if wanted == subscribed {
			self.queued = None;
			self.range_task = None;
		} else if self.queued != Some(wanted) {
			self.queued = Some(wanted);
			self.range_task = Some(cx.spawn(async move |this, cx| {
				cx.background_executor().timer(RANGE_DEBOUNCE).await;
				this.update(cx, |this, cx| this.request_range(wanted, cx)).ok();
			}));
		}
	}

	fn request_range(&mut self, wanted: Windows, cx: &mut Context<Self>) {
		self.queued = None;
		self.range_task = None;
		let Some(guild) = self.guild else {
			return;
		};
		if selected_channel(&self.data.read(cx).state(), guild) != Some(wanted.channel) {
			return;
		}
		self.subscribed = Some(wanted);
		self.data.read(cx).send(Command::MemberListRange {
			guild_id: guild,
			channel_id: wanted.channel,
			visible: wanted.rows(),
		});
	}

	fn on_search(&mut self, text: String, cx: &mut Context<Self>) {
		self.query = text.trim().to_lowercase();
		self.refresh(cx);
		let (Some(guild), false) = (self.guild, self.query.is_empty()) else {
			self.remote_search = None;
			return;
		};
		self.remote_search = Some(cx.spawn(async move |this, cx| {
			cx.background_executor().timer(SEARCH_DEBOUNCE).await;
			this.update(cx, |this, cx| this.request_remote(guild, cx)).ok();
		}));
	}

	fn request_remote(&mut self, guild: Snowflake, cx: &mut Context<Self>) {
		let incomplete = {
			let data = self.data.read(cx);
			let state = data.state();
			state.guild(guild).is_some_and(|found| {
				found
					.member_count
					.is_some_and(|total| total > found.members.len() as i64)
			})
		};
		if incomplete && self.guild == Some(guild) && !self.query.is_empty() {
			self.data.read(cx).send(Command::SearchMembers {
				guild_id: guild,
				query: self.query.clone(),
			});
		}
	}

	fn search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
		if let Some(input) = &self.input {
			if self.clear_input {
				self.clear_input = false;
				input.update(cx, |state, cx| state.set_value("", window, cx));
			}
			return input.clone();
		}
		let placeholder = cx.t("members-search");
		let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		self._subscriptions
			.push(cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
				if matches!(event, InputEvent::Change) {
					let text = input.read(cx).value().to_string();
					this.on_search(text, cx);
				}
			}));
		self.input = Some(input.clone());
		input
	}

	fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(input) = &self.input {
			input.update(cx, |state, cx| {
				state.set_value("", window, cx);
				state.focus(window, cx);
			});
		}
		self.on_search(String::new(), cx);
	}

	fn search_box(&self, search: &Entity<InputState>, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let trailing = if self.query.is_empty() {
			Icon::new(IconKind::Search)
				.size(theme.glyph_sm)
				.color(theme.text_muted)
				.into_any_element()
		} else {
			IconButton::new("member-search-clear", IconKind::Close)
				.icon_size(theme.glyph_sm)
				.hit_size(rem(SEARCH_HEIGHT - 6.))
				.tooltip(cx.t("members-search-clear"))
				.on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx)))
				.into_any_element()
		};
		div()
			.flex_none()
			.px(theme.space_8)
			.pt(theme.space_12)
			.child(
				div()
					.flex()
					.items_center()
					.h(rem(SEARCH_HEIGHT))
					.pl(theme.space_4)
					.pr(theme.space_4)
					.rounded(theme.radius_md)
					.border_1()
					.border_color(theme.divider)
					.bg(theme.background_tertiary)
					.child(
						Input::new(search)
							.appearance(false)
							.bordered(false)
							.focus_bordered(false)
							.small()
							.flex_1(),
					)
					.child(
						div()
							.flex()
							.flex_none()
							.items_center()
							.justify_center()
							.size(rem(SEARCH_HEIGHT - 6.))
							.child(trailing),
					),
			)
			.into_any_element()
	}

	fn row(
		&self,
		member: &MemberItem,
		bot_label: &SharedString,
		developer: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let offline = member.status == Status::Offline;
		let name_color = member.color.unwrap_or(if offline {
			theme.text_muted
		} else {
			theme.text_normal
		});
		let id = member.id;
		let data = self.data.clone();
		let (me, facts) = {
			let data = self.data.read(cx);
			(data.me(), relation_facts(&data.state(), id))
		};
		let own = me == Some(id);
		let menu = ContextMenuBuilder::new().item(cx.t("menu-profile"), Some(IconKind::Profile), {
			let weak = cx.weak_entity();
			move |window, cx| {
				let at = window.mouse_position();
				weak.update(cx, |_, cx| cx.emit(MemberListEvent::OpenProfile { user: id, at }))
					.ok();
			}
		});
		let menu = if own {
			menu
		} else {
			let menu = menu.item(cx.t("menu-message"), Some(IconKind::Chat), move |_, cx| {
				data.read(cx).send(Command::OpenDm { user_id: id });
			});
			let menu = if member.bot && facts.relation != Relation::Blocked {
				menu
			} else {
				primary_items(menu, id, facts.relation, cx)
			};
			manage_items(menu.separator(), id, &member.name, &facts, cx)
		};
		let menu = if developer {
			menu.separator()
				.item(cx.t("menu-copy-user-id"), Some(IconKind::Copy), move |_, _| {
					if let Err(error) = platform::clipboard::write_text(&id.to_string()) {
						tracing::warn!(%error, "could not copy the user id");
					}
				})
		} else {
			menu
		};
		let menu = match self.guild {
			Some(guild) => moderation::user_items(menu, guild, id, &member.name),
			None => menu,
		};
		let menu = menu
			.plugin_items("user-context-menu", MenuTarget::user(self.guild, id, me), cx)
			.build();
		div()
			.id(("member", id.get()))
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.member_row_height)
			.mx(theme.space_8)
			.px(theme.space_8)
			.rounded(theme.radius_md)
			.cursor_pointer()
			.hover(|style| style.bg(theme.modifier_hover))
			.when(offline, |row| row.opacity(0.5))
			.on_click(cx.listener(move |_, event: &gpui::ClickEvent, _, cx| {
				cx.emit(MemberListEvent::OpenProfile {
					user: id,
					at: event.position(),
				})
			}))
			.context_menu(menu)
			.child(
				Avatar::new(member.name.clone())
					.size(rem(32.))
					.url(member.avatar.clone())
					.status(member.status)
					.decoration(plugins::decoration_of(member.over.as_ref()))
					.cutout(theme.background_primary),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.min_w_0()
					.child(
						div()
							.flex()
							.items_center()
							.gap(theme.space_4)
							.child(
								div()
									.min_w_0()
									.truncate()
									.text_size(theme.font_md)
									.line_height(rem(20.))
									.font_weight(FontWeight::MEDIUM)
									.text_color(name_color)
									.child(member.name.clone()),
							)
							.when(member.bot, |name| {
								name.child(
									div()
										.flex_none()
										.px(rem(5.))
										.rounded(theme.radius_sm)
										.bg(theme.brand)
										.text_size(rem(10.))
										.line_height(rem(15.))
										.font_weight(FontWeight::BOLD)
										.text_color(theme.text_on_brand)
										.child(bot_label.clone()),
								)
							})
							.children(
								member
									.over
									.as_ref()
									.map_or_else(Vec::new, |over| plugins::tags(over, id, theme, cx)),
							)
							.when_some(member.badge.clone(), |name, badge| {
								name.child(
									div()
										.flex_none()
										.px(theme.space_4)
										.rounded(theme.radius_sm)
										.bg(theme.mention_background)
										.text_size(theme.font_tiny)
										.text_color(theme.mention_text)
										.child(badge),
								)
							}),
					)
					.when_some(member.activity.clone(), |column, activity| {
						column.child(
							div()
								.truncate()
								.text_size(theme.font_tiny)
								.line_height(rem(14.))
								.text_color(theme.text_muted)
								.child(activity),
						)
					}),
			)
			.into_any_element()
	}
}

fn member_item(
	state: &State,
	(guild, id): (Snowflake, Snowflake),
	display: String,
	color: Option<u32>,
	badge: Option<String>,
	status: Status,
	activity: Option<String>,
) -> MemberItem {
	MemberItem {
		id,
		name: display.into(),
		color: color.map(role_display_color),
		status,
		activity: activity.map(Into::into),
		avatar: plugins::avatar_url(state, Some(guild), id, AVATAR_PIXELS),
		badge: badge.map(Into::into),
		bot: state.user(id).is_some_and(|user| user.bot == Some(true)),
		over: state.user_override(Some(guild), id).cloned(),
	}
}

fn same_shape(left: &[(usize, Row)], right: &[(usize, Row)]) -> bool {
	left.len() == right.len()
		&& left
			.iter()
			.zip(right)
			.all(|((left_index, left_row), (right_index, right_row))| {
				left_index == right_index
					&& matches!(left_row, Row::Group { .. }) == matches!(right_row, Row::Group { .. })
			})
}

fn search_rows(state: &State, guild: Snowflake, query: &str) -> Vec<Row> {
	let Some(found) = state.guild(guild) else {
		return Vec::new();
	};
	let mut matches: Vec<(String, Snowflake)> = found
		.members
		.keys()
		.filter_map(|id| {
			let name = plugins::display_name(state, Some(guild), *id);
			let hit = name.to_lowercase().contains(query)
				|| state
					.user(*id)
					.is_some_and(|user| user.username.to_lowercase().contains(query));
			hit.then_some((name, *id))
		})
		.collect();
	matches.sort_by_cached_key(|(name, _)| name.to_lowercase());
	matches.truncate(SEARCH_LIMIT);
	let mut rows = vec![Row::Group {
		id: SEARCH_GROUP.to_owned(),
		name: String::new(),
		count: matches.len() as u32,
	}];
	rows.extend(matches.into_iter().map(|(name, id)| {
		Row::Member(Box::new(member_item(
			state,
			(guild, id),
			name,
			state.role_color(guild, id),
			state
				.user_override(Some(guild), id)
				.and_then(|over| over.badge.clone()),
			status_of(state, id),
			None,
		)))
	}));
	rows
}

impl MemberList {
	fn group_header(
		&self,
		id: &str,
		name: &str,
		count: u32,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let count = ArgValue::from(count as usize);
		let title = match id {
			SEARCH_GROUP => cx.t_args("members-search-results", &[("count", count)]),
			"online" => cx.t_args("members-online", &[("count", count)]),
			"offline" => cx.t_args("members-offline", &[("count", count)]),
			_ => cx.t_args(
				"members-role",
				&[("role", ArgValue::from(name.to_owned())), ("count", count)],
			),
		};
		div()
			.px(theme.space_16)
			.pt(theme.space_24)
			.pb(theme.space_4)
			.text_size(theme.font_tiny)
			.font_weight(FontWeight::BOLD)
			.text_color(theme.header_secondary)
			.child(title.to_uppercase())
			.into_any_element()
	}

	fn placeholder_row(theme: &Theme) -> AnyElement {
		div()
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.member_row_height)
			.mx(theme.space_8)
			.px(theme.space_8)
			.child(
				div()
					.flex_none()
					.size(rem(32.))
					.rounded_full()
					.bg(theme.background_tertiary),
			)
			.child(
				div()
					.w(rem(PLACEHOLDER_NAME_WIDTH))
					.h(rem(10.))
					.rounded(theme.radius_sm)
					.bg(theme.background_tertiary),
			)
			.into_any_element()
	}

	fn list_row(
		&self,
		index: usize,
		theme: &Theme,
		bot_label: &SharedString,
		developer: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let found = self
			.rows
			.binary_search_by_key(&index, |(row_index, _)| *row_index)
			.ok()
			.map(|position| &self.rows[position].1);
		let element = match found {
			Some(Row::Group { id, name, count }) => self.group_header(id, name, *count, theme, cx),
			Some(Row::Member(member)) => self.row(member, bot_label, developer, theme, cx),
			None => Self::placeholder_row(theme),
		};
		div().w_full().min_w_0().child(element).into_any_element()
	}
}

impl Render for MemberList {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Rc::new(Theme::current(cx));
		let search = self.search_input(window, cx);
		self.track_visible(&theme, cx);
		let header = plugins::contextual(
			"member-list-header",
			ViewContext {
				guild_id: self.guild.map(Snowflake::get),
				..ViewContext::default()
			},
			cx,
		);
		let plugin_sections = super::plugins::contextual(
			"member-list-section",
			ViewContext {
				guild_id: self.guild.map(Snowflake::get),
				..ViewContext::default()
			},
			cx,
		);
		let row_theme = Rc::clone(&theme);
		let bot_label = cx.t("badge-app");
		let developer = developer_mode(cx);
		let rows = list(
			self.scroll.clone(),
			cx.processor(move |this, index: usize, _, cx| {
				this.list_row(index, &row_theme, &bot_label, developer, cx)
			}),
		)
		.size_full()
		.min_h_0()
		.pb(theme.space_16);
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.member_list_width)
			.h_full()
			.bg(theme.background_primary)
			.border_l_1()
			.border_color(theme.divider)
			.when(!header.is_empty(), |list| {
				list.child(
					div()
						.flex()
						.flex_col()
						.flex_none()
						.gap(theme.space_4)
						.px(theme.space_8)
						.pt(theme.space_8)
						.children(header),
				)
			})
			.child(self.search_box(&search, &theme, cx))
			.when(!plugin_sections.is_empty(), |list| {
				list.child(
					div()
						.flex()
						.flex_col()
						.flex_none()
						.gap(theme.space_8)
						.px(theme.space_8)
						.pt(theme.space_8)
						.children(plugin_sections),
				)
			})
			.child(
				div().id("member-list").flex_1().min_h_0().child(
					div()
						.size_full()
						.min_h_0()
						.min_w_0()
						.child(rows)
						.vertical_scrollbar(&self.scroll),
				),
			)
	}
}
