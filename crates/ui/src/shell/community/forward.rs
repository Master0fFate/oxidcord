use std::rc::Rc;

use gpui::{
	AnyElement, App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, FontWeight,
	InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{ChannelType, Snowflake};
use session::Command;

use super::modal::{self, Close};
use crate::components::{ToggleHandler, switch_row};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind, channel_icon};
use crate::lookup::Lookup;
use crate::shell::message::{Plain, Prefs, preview};
use crate::shell::quick_switcher::fuzzy_score;
use crate::theme::Theme;

const MAX_RESULTS: usize = 12;
const PREVIEW_LENGTH: usize = 160;

struct Destination {
	channel: Snowflake,
	name: SharedString,
	context: Option<SharedString>,
	icon: IconKind,
}

pub struct ForwardPicker {
	data: Entity<Data>,
	source: Snowflake,
	message: Snowflake,
	preview: SharedString,
	destinations: Vec<Destination>,
	input: Entity<InputState>,
	comment: Entity<InputState>,
	silent: bool,
	query: String,
	_subscription: Subscription,
}

impl EventEmitter<Close> for ForwardPicker {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, source: Snowflake, message: Snowflake) {
	let view = cx.new(|cx| ForwardPicker::new(data, source, message, window, cx));
	let focus = view.read(cx).input.read(cx).focus_handle(cx);
	let title = cx.t("forward-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn can_receive(kind: ChannelType) -> bool {
	matches!(
		kind,
		ChannelType::GuildText
			| ChannelType::GuildAnnouncement
			| ChannelType::GuildVoice
			| ChannelType::GuildStageVoice
			| ChannelType::PublicThread
			| ChannelType::PrivateThread
			| ChannelType::AnnouncementThread
	)
}

fn destinations(state: &state::State) -> Vec<Destination> {
	let mut found = Vec::new();
	for summary in state.guild_summaries() {
		for node in state.guild_channel_tree(summary.id) {
			for channel in node
				.children
				.iter()
				.filter(|channel| !channel.locked && can_receive(channel.kind) && state.can_send(channel.id))
			{
				found.push(Destination {
					channel: channel.id,
					name: channel.name.clone().into(),
					context: Some(summary.name.clone().into()),
					icon: channel_icon(channel.kind),
				});
			}
		}
	}
	found.extend(state.dm_rows().into_iter().map(|row| Destination {
		channel: row.channel_id,
		name: row.name.into(),
		context: None,
		icon: if row.recipients.len() > 1 {
			IconKind::Group
		} else {
			IconKind::AtSign
		},
	}));
	found
}

impl ForwardPicker {
	fn new(
		data: Entity<Data>,
		source: Snowflake,
		message: Snowflake,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let (text, destinations) = {
			let guard = data.read(cx);
			let state = guard.state();
			let lookup = Lookup {
				state: &state,
				guild: state.channel_guild(source),
			};
			let plain = Plain {
				lookup: &lookup,
				h24: Prefs::load(cx).time_24h,
				cx,
			};
			let text = state
				.messages(source)
				.and_then(|cache| cache.by_id.get(&message))
				.map(|found| preview(found, PREVIEW_LENGTH, &plain))
				.unwrap_or_default();
			(text, destinations(&state))
		};
		let placeholder = cx.t("forward-placeholder");
		let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
		let comment_placeholder = cx.t("forward-comment");
		let comment = cx.new(|cx| InputState::new(window, cx).placeholder(comment_placeholder));
		let subscription =
			cx.subscribe_in(
				&input,
				window,
				|this, input, event: &InputEvent, _, cx| match event {
					InputEvent::Change => {
						this.query = input.read(cx).value().trim().to_owned();
						cx.notify();
					}
					InputEvent::PressEnter { .. } => {
						if let Some(channel) = this.matches().first().map(|found| found.channel) {
							this.forward(channel, cx);
						}
					}
					_ => {}
				},
			);
		Self {
			data,
			source,
			message,
			preview: text.into(),
			destinations,
			input,
			comment,
			silent: false,
			query: String::new(),
			_subscription: subscription,
		}
	}

	fn matches(&self) -> Vec<&Destination> {
		if self.query.is_empty() {
			return self.destinations.iter().take(MAX_RESULTS).collect();
		}
		let needle: Vec<char> = self.query.trim_start_matches('#').chars().collect();
		let mut scored: Vec<(i32, &Destination)> = self
			.destinations
			.iter()
			.filter_map(|found| Some((fuzzy_score(&needle, &found.name)?, found)))
			.collect();
		scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
		scored
			.into_iter()
			.take(MAX_RESULTS)
			.map(|(_, found)| found)
			.collect()
	}

	fn forward(&mut self, to_channel_id: Snowflake, cx: &mut Context<Self>) {
		let comment = self.comment.read(cx).value().trim().to_owned();
		self.data.read(cx).send(Command::ForwardMessage {
			channel_id: self.source,
			message_id: self.message,
			to_channel_id,
			comment: Some(comment).filter(|comment| !comment.is_empty()),
			silent: self.silent,
		});
		cx.emit(Close);
	}
}

impl Render for ForwardPicker {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let rows: Vec<AnyElement> = self
			.matches()
			.into_iter()
			.enumerate()
			.map(|(index, found)| {
				let channel = found.channel;
				div()
					.id(("forward-destination", index))
					.flex()
					.items_center()
					.gap(theme.space_8)
					.h(theme.field_height)
					.px(theme.space_8)
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.text_color(theme.interactive_normal)
					.hover(|style| {
						style
							.bg(theme.modifier_hover)
							.text_color(theme.interactive_active)
					})
					.on_click(cx.listener(move |this, _, _, cx| this.forward(channel, cx)))
					.child(Icon::new(found.icon).size(theme.icon_md).color(theme.text_muted))
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.child(found.name.clone()),
					)
					.children(found.context.clone().map(|context| {
						div()
							.flex_none()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(context)
					}))
					.into_any_element()
			})
			.collect();
		let empty = rows.is_empty();
		let weak = cx.weak_entity();
		let silent: ToggleHandler = Rc::new(move |value, _, cx| {
			weak.update(cx, |this, cx| {
				this.silent = value;
				cx.notify();
			})
			.ok();
		});
		div()
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.pt(theme.space_8)
			.when(!self.preview.is_empty(), |column| {
				column.child(
					div()
						.px(theme.space_12)
						.py(theme.space_8)
						.rounded(theme.radius_sm)
						.bg(theme.background_secondary)
						.text_size(theme.font_sm)
						.text_color(theme.text_muted)
						.line_clamp(3)
						.child(self.preview.clone()),
				)
			})
			.child(Input::new(&self.input))
			.child(switch_row(
				"forward-silent",
				cx.t("forward-silent"),
				None,
				self.silent,
				&theme,
				silent,
			))
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_2)
					.max_h(theme.list_max_height)
					.children(rows)
					.when(empty, |list| {
						list.child(
							div()
								.p(theme.space_8)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(cx.t("quick-switcher-empty")),
						)
					})
					.overflow_y_scrollbar()
					.id("forward-list"),
			)
			.child(Input::new(&self.comment))
	}
}
