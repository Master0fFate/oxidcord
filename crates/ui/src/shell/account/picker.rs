use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Disableable as _;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::dialog::DialogFooter;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::Snowflake;
use session::Command;

use crate::components::Avatar;
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::Theme;

const MAX_GROUP_MEMBERS: usize = 10;
const AVATAR_PIXELS: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
	Create,
	Add(Snowflake),
}

struct Candidate {
	id: Snowflake,
	name: SharedString,
	username: SharedString,
	avatar: String,
}

struct FriendPicker {
	data: Entity<Data>,
	purpose: Purpose,
	candidates: Vec<Candidate>,
	selected: Vec<Snowflake>,
	capacity: usize,
	filter: String,
	search: Entity<InputState>,
	_subscription: Subscription,
}

impl FriendPicker {
	fn new(data: Entity<Data>, purpose: Purpose, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let placeholder = cx.t("group-search");
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let subscription = cx.subscribe_in(&search, window, |this, input, event: &InputEvent, _, cx| {
			if matches!(event, InputEvent::Change) {
				this.filter = input.read(cx).value().trim().to_lowercase();
				cx.notify();
			}
		});
		let (candidates, capacity) = {
			let state = data.read(cx).state();
			let existing: Vec<Snowflake> = match purpose {
				Purpose::Create => Vec::new(),
				Purpose::Add(channel) => state
					.channel(channel)
					.map(|channel| channel.recipient_ids.clone())
					.unwrap_or_default(),
			};
			let members = match purpose {
				Purpose::Create => 1,
				Purpose::Add(_) => existing.len() + 1,
			};
			let candidates = state
				.friends()
				.into_iter()
				.filter(|friend| !existing.contains(&friend.user_id))
				.map(|friend| Candidate {
					id: friend.user_id,
					avatar: media::cdn::avatar_url(
						friend.user_id.get(),
						friend.avatar.as_deref(),
						None,
						AVATAR_PIXELS,
					),
					name: friend.display.into(),
					username: friend.username.into(),
				})
				.collect();
			(candidates, MAX_GROUP_MEMBERS.saturating_sub(members))
		};
		Self {
			data,
			purpose,
			candidates,
			selected: Vec::new(),
			capacity,
			filter: String::new(),
			search,
			_subscription: subscription,
		}
	}

	fn toggle(&mut self, id: Snowflake, cx: &mut Context<Self>) {
		if let Some(at) = self.selected.iter().position(|selected| *selected == id) {
			self.selected.remove(at);
		} else if self.selected.len() < self.capacity {
			self.selected.push(id);
		}
		cx.notify();
	}

	fn confirm(&self, cx: &App) {
		let data = self.data.read(cx);
		match self.purpose {
			Purpose::Create => data.send(Command::CreateGroupDm {
				recipient_ids: self.selected.clone(),
			}),
			Purpose::Add(channel_id) => {
				for user_id in &self.selected {
					data.send(Command::AddGroupRecipient {
						channel_id,
						user_id: *user_id,
					});
				}
			}
		}
	}

	fn row(&self, candidate: &Candidate, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = candidate.id;
		let checked = self.selected.contains(&id);
		let full = !checked && self.selected.len() >= self.capacity;
		div()
			.id(("group-candidate", id.get()))
			.flex()
			.items_center()
			.gap(theme.space_12)
			.h(theme.member_row_height)
			.px(theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.when(full, |row| row.opacity(0.5))
			.hover(|style| style.bg(theme.modifier_hover))
			.on_click(cx.listener(move |this, _, _, cx| this.toggle(id, cx)))
			.child(
				Avatar::new(candidate.name.clone())
					.size(theme.avatar_sm)
					.url(candidate.avatar.clone())
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
							.child(candidate.name.clone()),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(candidate.username.clone()),
					),
			)
			.child(Checkbox::new(("group-check", id.get())).checked(checked))
			.into_any_element()
	}
}

impl Render for FriendPicker {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let remaining = self.capacity.saturating_sub(self.selected.len());
		let rows: Vec<AnyElement> = self
			.candidates
			.iter()
			.filter(|candidate| {
				self.filter.is_empty()
					|| candidate.name.to_lowercase().contains(&self.filter)
					|| candidate.username.to_lowercase().contains(&self.filter)
			})
			.map(|candidate| self.row(candidate, &theme, cx))
			.collect();
		let empty = rows.is_empty();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.child(
				div()
					.text_size(theme.font_sm)
					.text_color(theme.header_secondary)
					.child(cx.t_args("group-remaining", &[("count", ArgValue::from(remaining))])),
			)
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
								.child(cx.t("group-no-friends")),
						)
					})
					.overflow_y_scrollbar()
					.id("group-candidates"),
			)
	}
}

fn open_picker(data: Entity<Data>, purpose: Purpose, window: &mut Window, cx: &mut App) {
	let picker = cx.new(|cx| FriendPicker::new(data, purpose, window, cx));
	let (title, confirm) = match purpose {
		Purpose::Create => ("group-create-title", "group-create"),
		Purpose::Add(_) => ("group-add-title", "group-add"),
	};
	window.open_dialog(cx, move |dialog, window, cx| {
		let submit = picker.clone();
		let none_selected = picker.read(cx).selected.is_empty();
		dialog
			.title(cx.t(title))
			.w(cx.global::<Theme>().modal_width.to_pixels(window.rem_size()))
			.child(picker.clone())
			.footer(
				DialogFooter::new()
					.child(
						Button::new("group-cancel")
							.label(cx.t("action-cancel"))
							.on_click(|_, window, cx| window.close_dialog(cx)),
					)
					.child(
						Button::new("group-confirm")
							.primary()
							.disabled(none_selected)
							.label(cx.t(confirm))
							.on_click(move |_, window, cx| {
								submit.read(cx).confirm(cx);
								window.close_dialog(cx);
							}),
					),
			)
	});
}

pub fn open_create(data: Entity<Data>, window: &mut Window, cx: &mut App) {
	open_picker(data, Purpose::Create, window, cx);
}

pub fn open_add(data: Entity<Data>, channel: Snowflake, window: &mut Window, cx: &mut App) {
	open_picker(data, Purpose::Add(channel), window, cx);
}

pub fn confirm_close(data: Entity<Data>, channel: Snowflake, group: bool, window: &mut Window, cx: &mut App) {
	let (title, body, action) = if group {
		("group-leave-title", "group-leave-body", "group-leave")
	} else {
		("dm-close-title", "dm-close-body", "dm-close")
	};
	window.open_alert_dialog(cx, move |alert, _, cx| {
		let data = data.clone();
		alert
			.title(cx.t(title))
			.description(cx.t(body))
			.confirm()
			.ok_text(cx.t(action))
			.ok_variant(gpui_component::button::ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				data.read(cx).send(Command::CloseDm { channel_id: channel });
				true
			})
	});
}
