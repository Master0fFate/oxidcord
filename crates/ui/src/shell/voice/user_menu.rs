use gpui::{
	AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
	Window, div, prelude::FluentBuilder as _,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use protocol::Snowflake;

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
	user: Snowflake,
	name: SharedString,
	volume: Entity<SliderState>,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<UserMenuEvent> for UserMenu {}

impl UserMenu {
	pub fn new(data: Entity<Data>, user: Snowflake, name: SharedString, cx: &mut Context<Self>) -> Self {
		let start = data.read(cx).user_volume(user) * FULL_PERCENT;
		let volume = cx.new(|_| {
			SliderState::new()
				.min(0.)
				.max(VOLUME_MAX_PERCENT)
				.step(1.)
				.default_value(start)
		});
		let slider = cx.subscribe(&volume, move |this, _, event: &SliderEvent, cx| {
			if let SliderEvent::Change(value) = event {
				let percent = value.end();
				this.data.update(cx, |data, cx| {
					data.set_user_volume(user, percent / FULL_PERCENT, cx)
				});
				cx.notify();
			}
		});
		let observer = cx.observe(&data, |_, _, cx| cx.notify());
		Self {
			data,
			user,
			name,
			volume,
			_subscriptions: vec![slider, observer],
		}
	}
}

impl Render for UserMenu {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let user = self.user;
		let (percent, silenced, is_me) = {
			let data = self.data.read(cx);
			(
				(data.user_volume(user) * FULL_PERCENT).round(),
				data.is_silenced(user),
				data.me() == Some(user),
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
				menu.child(
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
								.child(cx.t("voice-user-volume").to_uppercase())
								.child(
									div()
										.font_weight(FontWeight::NORMAL)
										.text_color(theme.text_muted)
										.child(format!("{percent}%")),
								),
						)
						.child(Slider::new(&self.volume)),
				)
				.child(
					Switch::new("voice-user-mute")
						.checked(silenced)
						.label(cx.t("voice-user-mute"))
						.on_change(move |_, _, cx| {
							data.update(cx, |data, cx| data.toggle_silence(user, cx));
						}),
				)
			})
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
