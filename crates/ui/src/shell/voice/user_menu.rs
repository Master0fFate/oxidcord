use gpui::{
	AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use protocol::Snowflake;
use state::State;

use super::seat_change;
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::Theme;

const VOLUME_MAX_PERCENT: f32 = 200.;
const FULL_PERCENT: f32 = 100.;

pub enum UserMenuEvent {
	Profile(Snowflake),
}

pub struct UserMenu {
	data: Entity<Data>,
	guild: Option<Snowflake>,
	user: Snowflake,
	name: SharedString,
	volume: Entity<SliderState>,
	stream_volume: Entity<SliderState>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<UserMenuEvent> for UserMenu {}

impl UserMenu {
	pub fn new(
		data: Entity<Data>,
		guild: Option<Snowflake>,
		user: Snowflake,
		name: SharedString,
		cx: &mut Context<Self>,
	) -> Self {
		let (start, stream_start) = {
			let data = data.read(cx);
			(
				data.user_volume(user) * FULL_PERCENT,
				data.stream_volume() * FULL_PERCENT,
			)
		};
		let volume = volume_state(start, cx);
		let stream_volume = volume_state(stream_start, cx);
		let slider = cx.subscribe(&volume, move |this, _, event: &SliderEvent, cx| {
			if let SliderEvent::Change(value) = event {
				let percent = value.end();
				this.data.update(cx, |data, cx| {
					data.set_user_volume(user, percent / FULL_PERCENT, cx)
				});
				cx.notify();
			}
		});
		let stream_slider = cx.subscribe(&stream_volume, |this, _, event: &SliderEvent, cx| {
			if let SliderEvent::Change(value) = event {
				let percent = value.end();
				this.data
					.update(cx, |data, cx| data.set_stream_volume(percent / FULL_PERCENT, cx));
				cx.notify();
			}
		});
		let observer = cx.observe(&data, |_, _, cx| cx.notify());
		Self {
			data,
			guild,
			user,
			name,
			volume,
			stream_volume,
			_subscriptions: vec![slider, stream_slider, observer],
		}
	}
}

fn volume_state(start: f32, cx: &mut Context<UserMenu>) -> Entity<SliderState> {
	cx.new(|_| {
		SliderState::new()
			.min(0.)
			.max(VOLUME_MAX_PERCENT)
			.step(1.)
			.default_value(start)
	})
}

fn watching_stream_of(state: &State, user: Snowflake) -> bool {
	state
		.voice()
		.and_then(|voice| voice.watching.as_deref())
		.is_some_and(|key| {
			state
				.stream_of(user)
				.is_some_and(|stream| stream.stream_key == key)
		})
}

fn volume_row(
	label: SharedString,
	percent: f32,
	slider: &Entity<SliderState>,
	theme: &Theme,
) -> impl IntoElement {
	div()
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.child(
			div()
				.flex()
				.items_center()
				.justify_between()
				.text_size(theme.font_tiny)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_secondary)
				.child(label.to_uppercase())
				.child(
					div()
						.font_weight(FontWeight::NORMAL)
						.text_color(theme.text_muted)
						.child(format!("{percent}%")),
				),
		)
		.child(Slider::new(slider))
}

impl Render for UserMenu {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let user = self.user;
		let (percent, stream_percent, silenced, is_me, watching, seat) = {
			let data = self.data.read(cx);
			let state = data.state();
			(
				(data.user_volume(user) * FULL_PERCENT).round(),
				(data.stream_volume() * FULL_PERCENT).round(),
				data.is_silenced(user),
				data.me() == Some(user),
				watching_stream_of(&state, user),
				self.guild.and_then(|guild| seat_change(&state, guild, user)),
			)
		};
		let data = self.data.clone();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_12)
			.w(theme.voice_menu_width)
			.p(theme.space_12)
			.rounded(theme.radius_md)
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_floating)
			.child(
				div()
					.truncate()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::SEMIBOLD)
					.text_color(theme.header_primary)
					.child(self.name.clone()),
			)
			.when(!is_me, |menu| {
				menu.child(volume_row(
					cx.t("voice-user-volume"),
					percent,
					&self.volume,
					&theme,
				))
				.when(watching, |menu| {
					menu.child(volume_row(
						cx.t("voice-stream-volume"),
						stream_percent,
						&self.stream_volume,
						&theme,
					))
				})
				.child(
					Switch::new("voice-user-mute")
						.checked(silenced)
						.label(cx.t("voice-user-mute"))
						.on_change(move |_, _, cx| {
							data.update(cx, |data, cx| data.toggle_silence(user, cx));
						}),
				)
			})
			.children(seat.map(|seat| {
				let data = self.data.clone();
				div()
					.id("voice-user-seat")
					.flex()
					.items_center()
					.gap(theme.space_8)
					.px(theme.space_8)
					.h(theme.voice_button)
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.hover(|style| style.bg(theme.modifier_hover))
					.on_click(move |_, _, cx| data.read(cx).send(seat.command()))
					.child(
						Icon::new(seat.icon)
							.size(theme.voice_icon_sm)
							.color(theme.interactive_normal),
					)
					.child(cx.t(seat.label))
			}))
			.child(
				div()
					.id("voice-user-profile")
					.flex()
					.items_center()
					.gap(theme.space_8)
					.px(theme.space_8)
					.h(theme.voice_button)
					.rounded(theme.radius_sm)
					.cursor_pointer()
					.text_size(theme.font_sm)
					.text_color(theme.text_normal)
					.hover(|style| style.bg(theme.modifier_hover))
					.on_click(cx.listener(move |_, _, _, cx| cx.emit(UserMenuEvent::Profile(user))))
					.child(
						Icon::new(IconKind::Profile)
							.size(theme.voice_icon_sm)
							.color(theme.interactive_normal),
					)
					.child(cx.t("menu-profile")),
			)
	}
}
