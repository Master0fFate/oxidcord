use gpui::{
	AnyElement, AppContext as _, Context, Entity, FontWeight, Hsla, IntoElement, ParentElement as _, Render,
	SharedString, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use session::Command;

use super::kit;
use crate::components::{Avatar, ContextMenuBuilder, badge};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::lookup::avatar_for;
use crate::model::role_color;
use crate::shell::community::access::{GuildAccess, MODERATION_PERMISSIONS};
use crate::shell::moderation::user_items;
use crate::theme::Theme;

const ROW_LIMIT: usize = 150;

struct Row {
	id: Snowflake,
	name: String,
	username: String,
	avatar: String,
	color: Option<Hsla>,
	roles: Vec<(String, Option<u32>)>,
}

pub struct MembersPage {
	data: Entity<Data>,
	guild: Snowflake,
	search: Entity<InputState>,
	_subscriptions: Vec<Subscription>,
}

impl MembersPage {
	pub fn new(data: Entity<Data>, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let placeholder = cx.t("members-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscriptions = vec![
			cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
				if matches!(event, DataEvent::Changed(_) | DataEvent::MediaReady) {
					cx.notify();
				}
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
			search,
			_subscriptions: subscriptions,
		}
	}

	fn rows(&self, query: &str, cx: &gpui::App) -> (Vec<Row>, usize) {
		let state = self.data.read(cx).state();
		let Some(guild) = state.guild(self.guild) else {
			return (Vec::new(), 0);
		};
		let mut rows: Vec<Row> = guild
			.members
			.iter()
			.filter_map(|(id, member)| {
				let name = state.display_name(Some(self.guild), *id);
				let username = state
					.user(*id)
					.map(|user| user.username.clone())
					.unwrap_or_default();
				let matches = query.is_empty()
					|| name.to_lowercase().contains(query)
					|| username.to_lowercase().contains(query);
				matches.then(|| Row {
					id: *id,
					color: state.role_color(self.guild, *id).map(role_color),
					roles: member
						.roles
						.iter()
						.filter_map(|role| guild.roles.get(role))
						.map(|role| {
							(
								role.name.clone(),
								u32::try_from(role.color).ok().filter(|c| *c != 0),
							)
						})
						.collect(),
					avatar: avatar_for(&state, *id, 64),
					name,
					username,
				})
			})
			.collect();
		let total = rows.len();
		rows.sort_by_key(|row| row.name.to_lowercase());
		rows.truncate(ROW_LIMIT);
		(rows, total)
	}

	fn row(&self, row: &Row, access: &GuildAccess, theme: &Theme, cx: &gpui::App) -> AnyElement {
		let menu = user_items(ContextMenuBuilder::new(), self.guild, row.id, &row.name);
		let has_actions = access.has_any(MODERATION_PERMISSIONS);
		div()
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.list_row_height + theme.space_8)
			.px(theme.space_12)
			.border_b_1()
			.border_color(theme.divider)
			.child(
				Avatar::new(row.name.clone())
					.size(theme.portrait_md)
					.url(row.avatar.clone()),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.w(theme.manage_grid_cell * 2.)
					.min_w_0()
					.child(
						div()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(row.color.unwrap_or(theme.header_primary))
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
				div()
					.flex()
					.flex_wrap()
					.flex_1()
					.min_w_0()
					.gap(theme.space_4)
					.children(row.roles.iter().map(|(name, color)| {
						badge(
							name.clone(),
							color.map_or(theme.interactive_normal, role_color),
							theme,
						)
					})),
			)
			.when(has_actions, |line| {
				line.child(
					Button::new(("member-more", row.id.get()))
						.ghost()
						.small()
						.icon(gpui_component::Icon::default().path(IconKind::More.path()))
						.tooltip(cx.t("message-more"))
						.dropdown_menu(menu.build()),
				)
			})
			.into_any_element()
	}
}

impl Render for MembersPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let query = self.search.read(cx).value().trim().to_lowercase();
		let (rows, total) = self.rows(&query, cx);
		let access = GuildAccess::of(&self.data.read(cx).state(), self.guild);
		let list: Vec<AnyElement> = rows
			.iter()
			.map(|row| self.row(row, &access, &theme, cx))
			.collect();
		let empty = list.is_empty();
		let caption: SharedString = cx.t_args("members-count", &[("count", ArgValue::from(total))]);
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(kit::title(cx.t("server-settings-members"), &theme))
			.child(
				div()
					.flex_none()
					.pb(theme.space_16)
					.child(Input::new(&self.search)),
			)
			.child(kit::count_caption(caption, &theme))
			.child(
				div()
					.flex()
					.flex_col()
					.pb(theme.space_32)
					.children(list)
					.when(empty, |column| {
						column.child(kit::empty(IconKind::Members, cx.t("members-empty"), &theme))
					})
					.overflow_y_scrollbar()
					.id("members-list")
					.flex_1()
					.min_h_0(),
			)
	}
}
