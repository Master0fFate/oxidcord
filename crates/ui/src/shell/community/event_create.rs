use std::rc::Rc;

use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone as _, Timelike as _, Utc};
use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::time_field::{HourCycle, TimePrecision};
use protocol::{ChannelType, ScheduledEvent, ScheduledEventKind, ScheduledEventStatus, Snowflake};
use session::Command;

use super::data_handle;
use super::modal::{self, Close};
use crate::components::{PickHandler, dialog_footer, field, segmented, select};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::theme::Theme;

const KINDS: [(&str, ScheduledEventKind); 3] = [
	("events-kind-voice", ScheduledEventKind::Voice),
	("events-kind-stage", ScheduledEventKind::StageInstance),
	("events-kind-external", ScheduledEventKind::External),
];
const NAME_LIMIT: usize = 100;
const DESCRIPTION_LIMIT: usize = 1000;
const DEFAULT_LENGTH_HOURS: i64 = 1;

struct Editing {
	id: Snowflake,
	status: ScheduledEventStatus,
}

pub struct CreateEvent {
	data: Entity<Data>,
	guild: Snowflake,
	editing: Option<Editing>,
	kind: usize,
	channel: usize,
	channels: Vec<(Snowflake, SharedString)>,
	name: Entity<InputState>,
	description: Entity<TextareaState>,
	location: Entity<InputState>,
	starts: Entity<DatePickerState>,
	ends: Entity<DatePickerState>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<Close> for CreateEvent {}

fn next_hour() -> NaiveDateTime {
	let later = Local::now() + Duration::hours(1);
	later
		.naive_local()
		.with_minute(0)
		.and_then(|time| time.with_second(0))
		.unwrap_or_else(|| later.naive_local())
}

fn picker(
	at: NaiveDateTime,
	window: &mut Window,
	cx: &mut Context<CreateEvent>,
) -> (Entity<DatePickerState>, Subscription) {
	let state = cx.new(|cx| {
		let mut state = DatePickerState::new(window, cx)
			.time_precision(TimePrecision::Minute)
			.hour_cycle(HourCycle::H12);
		state.set_date_time(at, window, cx);
		state
	});
	let subscription = cx.subscribe(&state, |_, _, _: &DatePickerEvent, cx| cx.notify());
	(state, subscription)
}

fn input_changed<T: EventEmitter<InputEvent>>(
	input: &Entity<T>,
	window: &mut Window,
	cx: &mut Context<CreateEvent>,
) -> Subscription {
	cx.subscribe_in(input, window, |_, _, event: &InputEvent, _, cx| {
		if matches!(event, InputEvent::Change) {
			cx.notify();
		}
	})
}

pub fn open(window: &mut Window, cx: &mut App, guild: Snowflake) {
	build(window, cx, guild, 0, None);
}

pub fn open_edit(window: &mut Window, cx: &mut App, event: &ScheduledEvent) {
	if let Some(kind) = KINDS.iter().position(|(_, kind)| *kind == event.entity_type) {
		build(window, cx, event.guild_id, kind, Some(event));
	}
}

fn local_time(time: DateTime<Utc>) -> NaiveDateTime {
	time.with_timezone(&Local).naive_local()
}

fn build(window: &mut Window, cx: &mut App, guild: Snowflake, kind: usize, event: Option<&ScheduledEvent>) {
	let data = data_handle(cx);
	let name_placeholder = cx.t("events-name");
	let description_placeholder = cx.t("events-description-placeholder");
	let location_placeholder = cx.t("events-location");
	let view = cx.new(|cx| {
		let name = cx.new(|cx| {
			let mut name = InputState::new(window, cx).placeholder(name_placeholder);
			if let Some(event) = event {
				name.set_value(event.name.clone(), window, cx);
			}
			name
		});
		let description = cx.new(|cx| {
			let mut description = TextareaState::new(window, cx)
				.auto_grow(2, 5)
				.placeholder(description_placeholder);
			if let Some(text) = event.and_then(|event| event.description.as_deref()) {
				description.set_value(text.to_owned(), window, cx);
			}
			description
		});
		let location = cx.new(|cx| {
			let mut location = InputState::new(window, cx).placeholder(location_placeholder);
			if let Some(text) = event.and_then(ScheduledEvent::location) {
				location.set_value(text.to_owned(), window, cx);
			}
			location
		});
		let start = event
			.and_then(|event| event.scheduled_start_time)
			.map_or_else(next_hour, local_time);
		let end = event
			.and_then(|event| event.scheduled_end_time)
			.map_or_else(|| start + Duration::hours(DEFAULT_LENGTH_HOURS), local_time);
		let (starts, starts_subscription) = picker(start, window, cx);
		let (ends, ends_subscription) = picker(end, window, cx);
		let subscriptions = vec![
			input_changed(&name, window, cx),
			input_changed(&location, window, cx),
			input_changed(&description, window, cx),
			starts_subscription,
			ends_subscription,
		];
		let channels = channels_of(&data, guild, KINDS[kind].1, cx);
		let channel = event
			.and_then(|event| event.channel_id)
			.and_then(|id| channels.iter().position(|(known, _)| *known == id))
			.unwrap_or(0);
		CreateEvent {
			data,
			guild,
			editing: event.map(|event| Editing {
				id: event.id,
				status: event.status,
			}),
			kind,
			channel,
			channels,
			name,
			description,
			location,
			starts,
			ends,
			_subscriptions: subscriptions,
		}
	});
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t(if event.is_some() {
		"events-edit"
	} else {
		"events-create"
	});
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn utc(local: NaiveDateTime) -> Option<DateTime<Utc>> {
	Local
		.from_local_datetime(&local)
		.earliest()
		.map(|time| time.with_timezone(&Utc))
}

fn picked(state: &Entity<DatePickerState>, cx: &App) -> Option<DateTime<Utc>> {
	state.read(cx).date_time().start().and_then(utc)
}

fn channels_of(
	data: &Entity<Data>,
	guild: Snowflake,
	kind: ScheduledEventKind,
	cx: &App,
) -> Vec<(Snowflake, SharedString)> {
	let wanted = match kind {
		ScheduledEventKind::Voice => ChannelType::GuildVoice,
		ScheduledEventKind::StageInstance => ChannelType::GuildStageVoice,
		_ => return Vec::new(),
	};
	let state = data.read(cx).state();
	let Some(guild) = state.guild(guild) else {
		return Vec::new();
	};
	let mut channels: Vec<_> = guild
		.channels
		.values()
		.filter(|channel| channel.kind == wanted)
		.collect();
	channels.sort_by_key(|channel| (channel.position.unwrap_or(0), channel.id));
	channels
		.into_iter()
		.map(|channel| (channel.id, channel.name.clone().unwrap_or_default().into()))
		.collect()
}

fn text(value: SharedString, limit: usize) -> Option<String> {
	let trimmed = value.trim();
	(!trimmed.is_empty()).then(|| trimmed.chars().take(limit).collect())
}

impl CreateEvent {
	fn started(&self) -> bool {
		self.editing
			.as_ref()
			.is_some_and(|editing| editing.status == ScheduledEventStatus::Active)
	}

	fn starts_at(&self, cx: &App) -> Option<DateTime<Utc>> {
		picked(&self.starts, cx).filter(|start| self.started() || *start > Utc::now())
	}

	fn can_submit(&self, cx: &App) -> bool {
		if self.name.read(cx).value().trim().is_empty() {
			return false;
		}
		let Some(starts_at) = self.starts_at(cx) else {
			return false;
		};
		if KINDS[self.kind].1 == ScheduledEventKind::External {
			picked(&self.ends, cx).is_some_and(|end| end > starts_at)
				&& !self.location.read(cx).value().trim().is_empty()
		} else {
			self.channel < self.channels.len()
		}
	}

	fn command(&self, cx: &App) -> Option<Command> {
		let (_, kind) = KINDS[self.kind];
		let name = text(self.name.read(cx).value(), NAME_LIMIT)?;
		let starts_at = self.starts_at(cx)?;
		let external = kind == ScheduledEventKind::External;
		let (channel_id, location, ends_at) = if external {
			let ends_at = picked(&self.ends, cx).filter(|end| *end > starts_at)?;
			let location = text(self.location.read(cx).value(), NAME_LIMIT)?;
			(None, Some(location), Some(ends_at))
		} else {
			let channel = self.channels.get(self.channel)?.0;
			(Some(channel), None, None)
		};
		let description = text(self.description.read(cx).value(), DESCRIPTION_LIMIT);
		Some(match &self.editing {
			Some(editing) => Command::EditScheduledEvent {
				guild_id: self.guild,
				event_id: editing.id,
				name,
				description,
				channel_id,
				location,
				starts_at: (!self.started()).then_some(starts_at),
				ends_at,
			},
			None => Command::CreateScheduledEvent {
				guild_id: self.guild,
				name,
				description,
				kind,
				channel_id,
				location,
				starts_at,
				ends_at,
			},
		})
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if let Some(command) = self.command(cx) {
			self.data.read(cx).send(command);
			cx.emit(Close);
		}
	}
}

impl Render for CreateEvent {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let kind_labels: Vec<SharedString> = KINDS.iter().map(|(key, _)| cx.t(key)).collect();
		let pick_kind: PickHandler = {
			let weak = cx.weak_entity();
			Rc::new(move |index, _, cx| {
				weak.update(cx, |this, cx| {
					this.kind = index;
					this.channel = 0;
					this.channels = channels_of(&this.data, this.guild, KINDS[index].1, cx);
					cx.notify();
				})
				.ok();
			})
		};
		let external = KINDS[self.kind].1 == ScheduledEventKind::External;
		let place = if external {
			field(cx.t("events-location"), &theme, Input::new(&self.location))
		} else {
			let labels: Vec<SharedString> = self.channels.iter().map(|(_, name)| name.clone()).collect();
			let current = labels
				.get(self.channel)
				.cloned()
				.unwrap_or_else(|| cx.t("events-channel-none"));
			let weak = cx.weak_entity();
			let pick_channel: PickHandler = Rc::new(move |index, _, cx| {
				weak.update(cx, |this, cx| {
					this.channel = index;
					cx.notify();
				})
				.ok();
			});
			field(
				cx.t("events-channel"),
				&theme,
				select("event-channel", current, &labels, self.channel, pick_channel),
			)
		};
		let weak_cancel = cx.weak_entity();
		let weak_create = cx.weak_entity();
		let pick_time = cx.t("events-pick-time");
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.children(self.editing.is_none().then(|| {
				field(
					cx.t("events-kind"),
					&theme,
					segmented("event-kind", &kind_labels, self.kind, &theme, pick_kind),
				)
			}))
			.child(field(cx.t("events-name"), &theme, Input::new(&self.name)))
			.child(place)
			.children((!self.started()).then(|| {
				field(
					cx.t("events-starts"),
					&theme,
					DatePicker::new(&self.starts).placeholder(pick_time.clone()),
				)
			}))
			.children(external.then(|| {
				field(
					cx.t("events-ends"),
					&theme,
					DatePicker::new(&self.ends).placeholder(pick_time),
				)
			}))
			.child(field(
				cx.t("events-description"),
				&theme,
				Textarea::new(&self.description),
			))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t(if self.editing.is_some() {
					"events-save"
				} else {
					"events-create-confirm"
				}),
				false,
				self.can_submit(cx),
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_create.update(cx, |this, cx| this.submit(cx)).ok();
				},
			))
	}
}
