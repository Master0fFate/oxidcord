use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FontWeight, IntoElement,
	ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{Disableable as _, Sizable as _};
use protocol::Snowflake;
use session::Command;

use crate::components::Avatar;
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::lookup::avatar_for;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const ROW_LIMIT: usize = 50;
const ADD_ACTION: &str = "add thread member";

struct Row {
	id: Snowflake,
	name: SharedString,
	username: SharedString,
	avatar: String,
}

pub struct InviteMembers {
	data: Entity<Data>,
	guild: Snowflake,
	thread: Snowflake,
	added: Vec<Snowflake>,
	search: Entity<InputState>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<Close> for InviteMembers {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, guild: Snowflake, thread: Snowflake) {
	let view = cx.new(|cx| InviteMembers::new(data, guild, thread, window, cx));
	let title = cx.t("thread-add-members");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, None);
}

impl InviteMembers {
	fn new(
		data: Entity<Data>,
		guild: Snowflake,
		thread: Snowflake,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let placeholder = cx.t("members-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscriptions = vec![
			cx.subscribe(&data, move |this, data, event: &DataEvent, cx| match event {
				DataEvent::Changed(changes) => {
					let failed = changes.rejected
						&& data
							.read(cx)
							.state()
							.last_rejection()
							.is_some_and(|rejection| rejection.reason.starts_with(ADD_ACTION));
					if failed {
						this.added.pop();
					}
					if failed || changes.members.contains(guild) {
						cx.notify();
					}
				}
				DataEvent::MediaReady => cx.notify(),
			}),
			cx.subscribe_in(
				&search,
				window,
				|this, input, event: &InputEvent, _, cx| match event {
					InputEvent::PressEnter { .. } => {
						let query = input.read(cx).value().trim().to_owned();
						if !query.is_empty() {
							this.data.read(cx).send(Command::SearchMembers {
								guild_id: this.guild,
								query,
							});
						}
					}
					InputEvent::Change => cx.notify(),
					_ => {}
				},
			),
		];
		Self {
			data,
			guild,
			thread,
			added: Vec::new(),
			search,
			_subscriptions: subscriptions,
		}
	}

	fn rows(&self, query: &str, cx: &App) -> Vec<Row> {
		let state = self.data.read(cx).state();
		let Some(guild) = state.guild(self.guild) else {
			return Vec::new();
		};
		let me = state.current_user_id();
		let mut found: Vec<(String, Snowflake, String, String)> = guild
			.members
			.keys()
			.filter(|id| Some(**id) != me)
			.filter_map(|id| {
				let name = state.display_name(Some(self.guild), *id);
				let username = state
					.user(*id)
					.map(|user| user.username.clone())
					.unwrap_or_default();
				let lowered = name.to_lowercase();
				let matches =
					query.is_empty() || lowered.contains(query) || username.to_lowercase().contains(query);
				matches.then_some((lowered, *id, name, username))
			})
			.collect();
		found.sort_unstable_by(|left, right| left.0.cmp(&right.0));
		found.truncate(ROW_LIMIT);
		found
			.into_iter()
			.map(|(_, id, name, username)| Row {
				id,
				avatar: avatar_for(&state, id, 64),
				name: name.into(),
				username: username.into(),
			})
			.collect()
	}

	fn add(&mut self, user: Snowflake, cx: &mut Context<Self>) {
		self.data.read(cx).send(Command::AddThreadMember {
			thread_id: self.thread,
			user_id: user,
		});
		self.added.push(user);
		cx.notify();
	}

	fn row(&self, row: &Row, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = row.id;
		let added = self.added.contains(&id);
		div()
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.member_row_height)
			.px(theme.space_8)
			.child(
				Avatar::new(row.name.clone())
					.size(theme.avatar_sm)
					.url(row.avatar.clone())
					.cutout(theme.background_floating),
			)
			.child(
				div()
					.flex()
					.flex_1()
					.min_w_0()
					.items_baseline()
					.gap(theme.space_8)
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::MEDIUM)
							.text_color(theme.header_primary)
							.child(row.name.clone()),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(row.username.clone()),
					),
			)
			.child(
				Button::new(("thread-invite", id.get()))
					.small()
					.outline()
					.disabled(added)
					.label(cx.t(if added { "thread-member-added" } else { "group-add" }))
					.on_click(cx.listener(move |this, _, _, cx| this.add(id, cx))),
			)
			.into_any_element()
	}
}

impl Render for InviteMembers {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let query = self.search.read(cx).value().trim().to_lowercase();
		let rows: Vec<AnyElement> = self
			.rows(&query, cx)
			.iter()
			.map(|row| self.row(row, &theme, cx))
			.collect();
		let empty = rows.is_empty();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.child(Input::new(&self.search))
			.child(
				div()
					.flex()
					.flex_col()
					.h(theme.list_max_height)
					.children(rows)
					.when(empty, |list| {
						list.child(
							div()
								.p(theme.space_8)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(cx.t("members-empty")),
						)
					})
					.overflow_y_scrollbar()
					.id("thread-invite-members"),
			)
	}
}
