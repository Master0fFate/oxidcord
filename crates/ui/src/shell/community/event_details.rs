use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render,
	Styled as _, Subscription, Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use protocol::Snowflake;

use super::access::GuildAccess;
use super::data_handle;
use super::events::{block, card};
use super::modal::{self, Close};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::theme::Theme;

pub struct EventDetails {
	data: Entity<Data>,
	guild: Snowflake,
	event: Snowflake,
	_subscription: Subscription,
}

impl EventEmitter<Close> for EventDetails {}

pub fn open(window: &mut Window, cx: &mut App, guild: Snowflake, event: Snowflake) {
	let data = data_handle(cx);
	let view = cx.new(|cx| {
		let subscription = cx.subscribe(&data, move |_, _, change: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = change
				&& (changes.loaded.contains(guild) || changes.channel_lists.contains(guild))
			{
				cx.notify();
			}
		});
		EventDetails {
			data,
			guild,
			event,
			_subscription: subscription,
		}
	});
	let title = cx.t("events-details");
	let width = cx.global::<Theme>().modal_width_md;
	modal::open(window, cx, title, width, &view, None);
}

impl Render for EventDetails {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let guild = self.guild;
		let event = self.event;
		let content = {
			let state = self.data.read(cx).state();
			let access = GuildAccess::of(&state, guild);
			state
				.scheduled_events(guild)
				.iter()
				.find(|known| known.id == event)
				.map(|known| block(guild, &card(known, &state, &access, cx), true, &theme, cx))
		};
		let weak = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(content.unwrap_or_else(|| {
				div()
					.p(theme.space_16)
					.text_size(theme.font_md)
					.text_color(theme.text_muted)
					.child(cx.t("events-gone"))
					.into_any_element()
			}))
			.child(
				div().flex().justify_end().child(
					Button::new("event-details-close")
						.ghost()
						.label(cx.t("action-close"))
						.on_click(move |_, _, cx| {
							weak.update(cx, |_, cx| cx.emit(Close)).ok();
						}),
				),
			)
	}
}
