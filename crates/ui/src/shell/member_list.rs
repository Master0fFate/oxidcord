use std::time::Duration;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, Hsla, InteractiveElement as _,
	IntoElement, ParentElement as _, Pixels, Point, Render, SharedString, StatefulInteractiveElement as _,
	Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::scroll::ScrollableElement as _;
use plugin_host::ViewContext;
use protocol::Snowflake;
use session::Command;
use state::{MemberListRow, State};

use super::community::developer_mode;
use super::moderation;
use super::plugins::{MenuTarget, PluginMenu as _};
use crate::components::{Avatar, ContextMenuBuilder, IconButton};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::lookup::avatar_for;
use crate::model::{Status, role_color, status_of};
use crate::theme::{Theme, rem};

const AVATAR_PIXELS: u32 = 64;
const SEARCH_LIMIT: usize = 100;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(350);
const SEARCH_GROUP: &str = "search";
const SEARCH_HEIGHT: f32 = 30.;

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
}

#[derive(PartialEq)]
enum Row {
	Group { id: String, name: String, count: u32 },
	Member(MemberItem),
}

pub struct MemberList {
	data: Entity<Data>,
	guild: Option<Snowflake>,
	rows: Vec<Row>,
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
		self.guild = guild;
		self.refresh(cx);
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let Some(guild) = self.guild else {
			return;
		};
		let data = self.data.read(cx);
		let state = data.state();
		let rows = if self.query.is_empty() {
			state
				.member_rows(guild)
				.into_iter()
				.map(|row| match row {
					MemberListRow::Group { id, name, count } => Row::Group { id, name, count },
					MemberListRow::Member(member) => Row::Member(member_item(
						&state,
						member.user_id,
						member.display,
						member.color,
						member.badge,
						Status::from(&member.status),
						member.activity,
					)),
				})
				.collect()
		} else {
			search_rows(&state, guild, &self.query)
		};
		drop(state);
		if rows != self.rows {
			self.rows = rows;
			cx.notify();
		}
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
		let menu = ContextMenuBuilder::new()
			.item(cx.t("menu-profile"), Some(IconKind::Profile), {
				let weak = cx.weak_entity();
				move |window, cx| {
					let at = window.mouse_position();
					weak.update(cx, |_, cx| cx.emit(MemberListEvent::OpenProfile { user: id, at }))
						.ok();
				}
			})
			.item(cx.t("menu-message"), Some(IconKind::Chat), move |_, cx| {
				data.read(cx).send(Command::OpenDm { user_id: id });
			})
			.disabled_item(cx.t("menu-add-friend"), Some(IconKind::FriendRequests));
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
		let me = self.data.read(cx).me();
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
	id: Snowflake,
	display: String,
	color: Option<u32>,
	badge: Option<String>,
	status: Status,
	activity: Option<String>,
) -> MemberItem {
	MemberItem {
		id,
		name: display.into(),
		color: color.map(role_color),
		status,
		activity: activity.map(Into::into),
		avatar: avatar_for(state, id, AVATAR_PIXELS),
		badge: badge.map(Into::into),
		bot: state.user(id).is_some_and(|user| user.bot == Some(true)),
	}
}

fn search_rows(state: &State, guild: Snowflake, query: &str) -> Vec<Row> {
	let Some(found) = state.guild(guild) else {
		return Vec::new();
	};
	let mut matches: Vec<(String, Snowflake)> = found
		.members
		.keys()
		.filter_map(|id| {
			let name = state.display_name(Some(guild), *id);
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
		Row::Member(member_item(
			state,
			id,
			name,
			state.role_color(guild, id),
			state
				.user_override(Some(guild), id)
				.and_then(|over| over.badge.clone()),
			status_of(state, id),
			None,
		))
	}));
	rows
}

impl Render for MemberList {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let search = self.search_input(window, cx);
		let developer = developer_mode(cx);
		let bot_label = cx.t("badge-bot");
		let mut sections: Vec<AnyElement> = Vec::new();
		for row in &self.rows {
			match row {
				Row::Group { id, name, count } => {
					let count = ArgValue::from(*count as usize);
					let title = match id.as_str() {
						SEARCH_GROUP => cx.t_args("members-search-results", &[("count", count)]),
						"online" => cx.t_args("members-online", &[("count", count)]),
						"offline" => cx.t_args("members-offline", &[("count", count)]),
						_ => cx.t_args(
							"members-role",
							&[("role", ArgValue::from(name.clone())), ("count", count)],
						),
					};
					sections.push(
						div()
							.px(theme.space_16)
							.pt(theme.space_24)
							.pb(theme.space_4)
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(title.to_uppercase())
							.into_any_element(),
					);
				}
				Row::Member(member) => sections.push(self.row(member, &bot_label, developer, &theme, cx)),
			}
		}
		let plugin_sections = super::plugins::contextual(
			"member-list-section",
			ViewContext {
				guild_id: self.guild.map(Snowflake::get),
				..ViewContext::default()
			},
			cx,
		);
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.member_list_width)
			.h_full()
			.bg(theme.background_primary)
			.border_l_1()
			.border_color(theme.divider)
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
				div()
					.flex()
					.flex_col()
					.pb(theme.space_16)
					.children(sections)
					.overflow_y_scrollbar()
					.id("member-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
