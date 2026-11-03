use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
	Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Permissions, ScheduledEvent, ScheduledEventKind, ScheduledEventStatus, Snowflake};
use session::Command;
use state::State;

use super::access::GuildAccess;
use super::data_handle;
use super::event_create;
use super::event_details;
use super::modal::{self, Close};
use crate::components::{IconButton, badge, dialog_footer};
use crate::data::{Data, DataEvent};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind, channel_icon};
use crate::theme::{Theme, rem};

pub const CREATE_PERMISSIONS: Permissions = Permissions::CREATE_EVENTS.union(Permissions::MANAGE_EVENTS);

pub(super) struct EventCard {
	pub id: Snowflake,
	pub name: SharedString,
	pub description: Option<SharedString>,
	pub when: SharedString,
	pub ends: Option<SharedString>,
	pub status: ScheduledEventStatus,
	pub repeats: bool,
	pub place_icon: IconKind,
	pub place: SharedString,
	pub joinable: Option<Snowflake>,
	pub interested_count: i64,
	pub interested: bool,
	pub can_manage: bool,
}

pub struct EventsList {
	data: Entity<Data>,
	guild: Snowflake,
	_subscription: Subscription,
}

impl EventEmitter<Close> for EventsList {}

pub fn open_details(window: &mut Window, cx: &mut App, guild: Snowflake, event: Snowflake) {
	event_details::open(window, cx, guild, event);
}

pub fn open(window: &mut Window, cx: &mut App, guild: Snowflake) {
	let data = data_handle(cx);
	data.read(cx)
		.send(Command::LoadScheduledEvents { guild_id: guild });
	let view = cx.new(|cx| {
		let subscription = cx.subscribe(&data, move |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.loaded.contains(guild)
					|| changes.channel_lists.contains(guild)
					|| changes.guild_list)
			{
				cx.notify();
			}
		});
		EventsList {
			data,
			guild,
			_subscription: subscription,
		}
	});
	let title = cx.t("events-title");
	let width = cx.global::<Theme>().modal_width_md;
	modal::open(window, cx, title, width, &view, None);
}

fn place(event: &ScheduledEvent, state: &State) -> (IconKind, SharedString) {
	if event.entity_type == ScheduledEventKind::External {
		return (
			IconKind::ExternalLink,
			event.location().unwrap_or_default().to_owned().into(),
		);
	}
	let channel = event.channel_id.and_then(|id| state.channel(id));
	(
		channel.map_or(IconKind::Volume, |channel| channel_icon(channel.kind)),
		channel
			.and_then(|channel| channel.name.clone())
			.unwrap_or_default()
			.into(),
	)
}

fn when(event: &ScheduledEvent, cx: &App) -> SharedString {
	if event.status == ScheduledEventStatus::Active {
		cx.t("voice-live")
	} else {
		event
			.scheduled_start_time
			.map(|start| format::event_time(start, cx))
			.unwrap_or_default()
	}
}

pub(super) fn summary(event: &ScheduledEvent, state: &State, theme: &Theme, cx: &App) -> AnyElement {
	let (place_icon, place) = place(event, state);
	let live = event.status == ScheduledEventStatus::Active;
	div()
		.flex()
		.flex_col()
		.gap(theme.space_4)
		.p(theme.space_12)
		.rounded(theme.radius_md)
		.bg(theme.background_tertiary)
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(Icon::new(IconKind::Calendar).size(rem(16.)).color(theme.brand))
				.child(if live {
					badge(when(event, cx), theme.danger, theme)
				} else {
					div()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::BOLD)
						.text_color(theme.brand)
						.child(when(event, cx))
						.into_any_element()
				}),
		)
		.child(
			div()
				.truncate()
				.text_size(theme.font_md)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_primary)
				.child(event.name.clone()),
		)
		.when(!place.is_empty(), |summary| {
			summary.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(
						Icon::new(place_icon)
							.size(rem(16.))
							.color(theme.interactive_muted),
					)
					.child(div().min_w_0().truncate().child(place)),
			)
		})
		.child(
			div()
				.text_size(theme.font_sm)
				.text_color(theme.text_muted)
				.child(cx.t_args(
					"events-interested-count",
					&[("count", ArgValue::from(event.user_count.unwrap_or(0)))],
				)),
		)
		.into_any_element()
}

pub(super) fn card(event: &ScheduledEvent, state: &State, access: &GuildAccess, cx: &App) -> EventCard {
	let (place_icon, place) = place(event, state);
	let own = event.creator_id.is_some() && event.creator_id == state.current_user_id();
	let live = event.status == ScheduledEventStatus::Active;
	EventCard {
		id: event.id,
		name: event.name.clone().into(),
		description: event
			.description
			.as_deref()
			.filter(|text| !text.is_empty())
			.map(|text| text.to_owned().into()),
		when: when(event, cx),
		ends: event.scheduled_end_time.map(|end| format::event_time(end, cx)),
		status: event.status,
		repeats: event.recurrence_rule.is_some(),
		place_icon,
		place,
		joinable: event
			.channel_id
			.filter(|_| live && event.entity_type != ScheduledEventKind::External),
		interested_count: event.user_count.unwrap_or(0),
		interested: state.event_interested(event.guild_id, event.id),
		can_manage: access.has(Permissions::MANAGE_EVENTS) || (own && access.has(Permissions::CREATE_EVENTS)),
	}
}

fn confirm_cancel(window: &mut Window, cx: &mut App, guild: Snowflake, event: Snowflake, name: SharedString) {
	let data = data_handle(cx);
	window.open_alert_dialog(cx, move |alert, _, cx| {
		let data = data.clone();
		alert
			.title(cx.t("events-cancel-title"))
			.description(cx.t_args(
				"events-cancel-body",
				&[("name", ArgValue::from(name.to_string()))],
			))
			.confirm()
			.ok_text(cx.t("events-cancel-confirm"))
			.ok_variant(ButtonVariant::Danger)
			.cancel_text(cx.t("action-cancel"))
			.on_ok(move |_, _, cx| {
				data.read(cx).send(Command::DeleteScheduledEvent {
					guild_id: guild,
					event_id: event,
				});
				true
			})
	});
}

fn edit_event(window: &mut Window, cx: &mut App, guild: Snowflake, event: Snowflake) {
	let found = data_handle(cx)
		.read(cx)
		.state()
		.scheduled_events(guild)
		.iter()
		.find(|known| known.id == event)
		.cloned();
	if let Some(found) = found {
		event_create::open_edit(window, cx, &found);
	}
}

pub(super) fn controls(guild: Snowflake, card: &EventCard, theme: &Theme, cx: &App) -> AnyElement {
	let id = card.id;
	let name = card.name.clone();
	let interested = card.interested;
	let toggle = Button::new(("event-interest", id.get()))
		.label(cx.t("events-interested"))
		.on_click(move |_, _, cx| {
			data_handle(cx).read(cx).send(Command::SetScheduledEventInterest {
				guild_id: guild,
				event_id: id,
				interested: !interested,
			});
		});
	let toggle = if interested {
		toggle.primary()
	} else {
		toggle.outline()
	};
	let join = card.joinable.map(|channel| {
		Button::new(("event-join", id.get()))
			.success()
			.label(cx.t("events-join"))
			.on_click(move |_, _, cx| {
				data_handle(cx).read(cx).send(Command::JoinVoice {
					guild_id: Some(guild),
					channel_id: channel,
				});
			})
	});
	let advance = match card.status {
		ScheduledEventStatus::Scheduled => Some(("events-start", ScheduledEventStatus::Active)),
		ScheduledEventStatus::Active => Some(("events-end", ScheduledEventStatus::Completed)),
		_ => None,
	}
	.filter(|_| card.can_manage)
	.map(|(label, status)| {
		Button::new(("event-advance", id.get()))
			.outline()
			.label(cx.t(label))
			.on_click(move |_, _, cx| {
				data_handle(cx).read(cx).send(Command::SetScheduledEventStatus {
					guild_id: guild,
					event_id: id,
					status,
				});
			})
	});
	let edit = card.can_manage.then(|| {
		IconButton::new(("event-edit", id.get()), IconKind::Edit)
			.tooltip(cx.t("events-edit"))
			.on_click(move |_, window, cx| edit_event(window, cx, guild, id))
	});
	let cancel = (card.can_manage && card.status == ScheduledEventStatus::Scheduled).then(|| {
		IconButton::new(("event-cancel", id.get()), IconKind::Trash)
			.color(theme.danger)
			.tooltip(cx.t("events-cancel"))
			.on_click(move |_, window, cx| confirm_cancel(window, cx, guild, id, name.clone()))
	});
	div()
		.flex()
		.items_center()
		.gap(theme.space_8)
		.child(toggle)
		.children(join)
		.children(advance)
		.children(edit)
		.children(cancel)
		.into_any_element()
}

pub(super) fn block(
	guild: Snowflake,
	card: &EventCard,
	detailed: bool,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	let live = card.status == ScheduledEventStatus::Active;
	let id = card.id;
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.p(theme.space_16)
		.rounded(theme.radius_md)
		.bg(theme.background_secondary)
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(if live {
					badge(card.when.clone(), theme.danger, theme)
				} else {
					div()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::BOLD)
						.text_color(theme.brand)
						.child(card.when.clone())
						.into_any_element()
				})
				.when(card.repeats, |header| {
					header.child(badge(cx.t("events-repeats"), theme.text_muted, theme))
				}),
		)
		.child(
			div()
				.id(("event-name", id.get()))
				.truncate()
				.text_size(theme.font_md)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_primary)
				.when(!detailed, |name| {
					name.cursor_pointer()
						.hover(|style| style.underline())
						.on_click(move |_, window, cx| open_details(window, cx, guild, id))
				})
				.child(card.name.clone()),
		)
		.children(card.description.clone().map(|text| {
			div()
				.text_size(theme.font_sm)
				.text_color(theme.text_muted)
				.when(!detailed, |text| text.line_clamp(2))
				.child(text)
		}))
		.when(!card.place.is_empty(), |row| {
			row.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(
						Icon::new(card.place_icon)
							.size(rem(16.))
							.color(theme.interactive_muted),
					)
					.child(div().min_w_0().truncate().child(card.place.clone())),
			)
		})
		.when_some(card.ends.clone().filter(|_| detailed), |row, ends| {
			row.child(
				div()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t_args("events-until", &[("time", ArgValue::from(ends.to_string()))])),
			)
		})
		.child(
			div()
				.flex()
				.items_center()
				.justify_between()
				.gap(theme.space_8)
				.child(
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_muted)
						.child(cx.t_args(
							"events-interested-count",
							&[("count", ArgValue::from(card.interested_count))],
						)),
				)
				.child(controls(guild, card, theme, cx)),
		)
		.into_any_element()
}

impl Render for EventsList {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let guild = self.guild;
		let (cards, can_create) = {
			let state = self.data.read(cx).state();
			let access = GuildAccess::of(&state, guild);
			let cards: Vec<EventCard> = state
				.scheduled_events(guild)
				.iter()
				.map(|event| card(event, &state, &access, cx))
				.collect();
			(cards, access.has_any(CREATE_PERMISSIONS))
		};
		let rows: Vec<AnyElement> = cards
			.iter()
			.map(|card| block(guild, card, false, &theme, cx))
			.collect();
		let empty = rows.is_empty();
		let weak = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.max_h(theme.list_max_height * 2.)
					.children(rows)
					.when(empty, |list| {
						list.child(
							div()
								.p(theme.space_16)
								.text_size(theme.font_md)
								.text_color(theme.text_muted)
								.child(cx.t("events-empty")),
						)
					})
					.overflow_y_scrollbar()
					.id("events-list"),
			)
			.child(dialog_footer(
				cx.t("action-close"),
				cx.t("events-create"),
				false,
				can_create,
				&theme,
				move |_, cx| {
					weak.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |window, cx| event_create::open(window, cx, guild),
			))
	}
}
