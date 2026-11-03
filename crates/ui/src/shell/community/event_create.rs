use std::rc::Rc;

use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone as _, Timelike as _, Utc};
use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::time_field::{HourCycle, TimePrecision};
use protocol::{ChannelType, ScheduledEventKind, Snowflake};
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

pub struct CreateEvent {
	data: Entity<Data>,
	guild: Snowflake,
	kind: usize,
	channel: usize,
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

fn input_changed(
	input: &Entity<InputState>,
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
	let data = data_handle(cx);
	let name_placeholder = cx.t("events-name");
	let description_placeholder = cx.t("events-description-placeholder");
	let location_placeholder = cx.t("events-location");
	let view = cx.new(|cx| {
		let name = cx.new(|cx| InputState::new(window, cx).placeholder(name_placeholder));
		let description = cx.new(|cx| {
			TextareaState::new(window, cx)
				.auto_grow(2, 5)
				.placeholder(description_placeholder)
		});
		let location = cx.new(|cx| InputState::new(window, cx).placeholder(location_placeholder));
		let start = next_hour();
		let (starts, starts_subscription) = picker(start, window, cx);
		let (ends, ends_subscription) = picker(start + Duration::hours(DEFAULT_LENGTH_HOURS), window, cx);
		let subscriptions = vec![
			input_changed(&name, window, cx),
			input_changed(&location, window, cx),
			cx.subscribe_in(&description, window, |_, _, event: &InputEvent, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
			starts_subscription,
			ends_subscription,
		];
		CreateEvent {
			data,
			guild,
			kind: 0,
			channel: 0,
			name,
			description,
			location,
			starts,
			ends,
			_subscriptions: subscriptions,
		}
	});
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t("events-create");
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

fn text(value: SharedString, limit: usize) -> Option<String> {
	let trimmed = value.trim();
	(!trimmed.is_empty()).then(|| trimmed.chars().take(limit).collect())
}

impl CreateEvent {
	fn channels(&self, cx: &App) -> Vec<(Snowflake, SharedString)> {
		let wanted = match KINDS[self.kind].1 {
			ScheduledEventKind::Voice => ChannelType::GuildVoice,
			ScheduledEventKind::StageInstance => ChannelType::GuildStageVoice,
			_ => return Vec::new(),
		};
		let state = self.data.read(cx).state();
		let Some(guild) = state.guild(self.guild) else {
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

	fn command(&self, cx: &App) -> Option<Command> {
		let (_, kind) = KINDS[self.kind];
		let name = text(self.name.read(cx).value(), NAME_LIMIT)?;
		let starts_at = picked(&self.starts, cx).filter(|start| *start > Utc::now())?;
		let external = kind == ScheduledEventKind::External;
		let (channel_id, location, ends_at) = if external {
			let ends_at = picked(&self.ends, cx).filter(|end| *end > starts_at)?;
			let location = text(self.location.read(cx).value(), NAME_LIMIT)?;
			(None, Some(location), Some(ends_at))
		} else {
			let channel = self.channels(cx).get(self.channel)?.0;
			(Some(channel), None, None)
		};
		Some(Command::CreateScheduledEvent {
			guild_id: self.guild,
			name,
			description: text(self.description.read(cx).value(), DESCRIPTION_LIMIT),
			kind,
			channel_id,
			location,
			starts_at,
			ends_at,
		})
	}

	fn create(&mut self, cx: &mut Context<Self>) {
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
					cx.notify();
				})
				.ok();
			})
		};
		let external = KINDS[self.kind].1 == ScheduledEventKind::External;
		let place = if external {
			field(cx.t("events-location"), &theme, Input::new(&self.location))
		} else {
			let channels = self.channels(cx);
			let labels: Vec<SharedString> = channels.iter().map(|(_, name)| name.clone()).collect();
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
			.child(field(
				cx.t("events-kind"),
				&theme,
				segmented("event-kind", &kind_labels, self.kind, &theme, pick_kind),
			))
			.child(field(cx.t("events-name"), &theme, Input::new(&self.name)))
			.child(place)
			.child(field(
				cx.t("events-starts"),
				&theme,
				DatePicker::new(&self.starts).placeholder(pick_time.clone()),
			))
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
				cx.t("events-create-confirm"),
				false,
				self.command(cx).is_some(),
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_create.update(cx, |this, cx| this.create(cx)).ok();
				},
			))
	}
}
