use std::time::Duration;

use gpui::{
	Anchor, AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FontWeight,
	InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Subscription, Window, anchored, deferred, div, img, point,
	prelude::FluentBuilder as _,
};
use media::SizeHint;
use protocol::ActivityKind;

use super::account::{StatusMenu, StatusMenuEvent};
use crate::components::{Avatar, IconButton, tooltip, unicode_emoji};
use crate::data::{Data, streamer_mode};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::IconKind;
use crate::images::Images;
use crate::lookup::user_avatar;
use crate::model::Status;
use crate::theme::{Theme, rem};

const AVATAR_PIXELS: u32 = 64;
const STATUS_EMOJI_SIZE: f32 = 14.;

#[derive(Clone, PartialEq)]
enum StatusEmoji {
	Unicode(SharedString),
	Custom { id: u64, animated: bool },
}

impl StatusEmoji {
	fn from_activity(emoji: &protocol::PartialEmoji) -> Option<Self> {
		match (emoji.id, &emoji.name) {
			(Some(id), _) => Some(Self::Custom {
				id: id.get(),
				animated: emoji.animated,
			}),
			(None, Some(name)) if !name.is_empty() => Some(Self::Unicode(name.clone().into())),
			_ => None,
		}
	}

	fn element(&self, cx: &App) -> AnyElement {
		match self {
			Self::Unicode(glyph) => unicode_emoji(glyph, rem(STATUS_EMOJI_SIZE), cx),
			Self::Custom { id, animated } => {
				let pixels = STATUS_EMOJI_SIZE as u32 * 2;
				let url = media::cdn::emoji_url(*id, *animated, pixels);
				let image = cx.global::<Images>().get(&url, SizeHint::square(pixels));
				div()
					.flex_none()
					.size(rem(STATUS_EMOJI_SIZE))
					.when_some(image, |slot, image| slot.child(img(image).size_full()))
					.into_any_element()
			}
		}
	}
}

pub enum UserPanelEvent {
	OpenSettings,
	EditProfile,
	LogOut,
}

pub struct UserPanel {
	data: Entity<Data>,
	name: SharedString,
	handle: SharedString,
	avatar: String,
	status: Status,
	custom_status: Option<SharedString>,
	custom_emoji: Option<StatusEmoji>,
	latency: Option<Duration>,
	menu: Option<(Entity<StatusMenu>, Point<Pixels>)>,
	_menu_events: Option<Subscription>,
	_voice_state: Subscription,
}

impl EventEmitter<UserPanelEvent> for UserPanel {}

impl UserPanel {
	fn toggle_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
		if self.menu.take().is_some() {
			self._menu_events = None;
			cx.notify();
			return;
		}
		let data = self.data.clone();
		let menu = cx.new(|cx| StatusMenu::new(data, cx));
		self._menu_events = Some(cx.subscribe(&menu, |this, _, event: &StatusMenuEvent, cx| {
			this.menu = None;
			this._menu_events = None;
			match event {
				StatusMenuEvent::Close => {}
				StatusMenuEvent::EditProfile => cx.emit(UserPanelEvent::EditProfile),
				StatusMenuEvent::LogOut => cx.emit(UserPanelEvent::LogOut),
			}
			cx.notify();
		}));
		self.menu = Some((menu, at));
		cx.notify();
	}

	fn menu_element(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
		let (menu, at) = self.menu.clone()?;
		let raised = point(at.x, at.y - theme.pixels(theme.space_20));
		Some(
			deferred(
				anchored()
					.position(raised)
					.anchor(Anchor::BottomLeft)
					.snap_to_window_with_margin(theme.pixels(theme.space_8))
					.child(
						div()
							.occlude()
							.on_mouse_down_out(cx.listener(|this, _, _, cx| {
								this.menu = None;
								this._menu_events = None;
								cx.notify();
							}))
							.child(menu),
					),
			)
			.with_priority(2)
			.into_any_element(),
		)
	}

	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let voice_state = cx.observe(&data, |_, _, cx| cx.notify());
		let mut panel = Self {
			data,
			name: SharedString::default(),
			handle: SharedString::default(),
			avatar: String::new(),
			status: Status::Online,
			custom_status: None,
			custom_emoji: None,
			latency: None,
			menu: None,
			_menu_events: None,
			_voice_state: voice_state,
		};
		panel.refresh(cx);
		panel
	}

	pub fn refresh(&mut self, cx: &mut Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		let latency = state.session().latency;
		let Some(user) = state.current_user() else {
			return;
		};
		let presence = state.presence(user.id);
		let name: SharedString = state.display_name(None, user.id).into();
		let handle: SharedString = user.username.clone().into();
		let avatar = user_avatar(user, AVATAR_PIXELS);
		let status = Status::from(&state.own_status());
		let custom = presence.and_then(|presence| {
			presence
				.activities
				.iter()
				.find(|activity| activity.kind == ActivityKind::Custom)
		});
		let custom_status: Option<SharedString> = custom
			.and_then(|activity| activity.state.clone())
			.filter(|text| !text.is_empty())
			.map(Into::into);
		let custom_emoji = custom
			.and_then(|activity| activity.emoji.as_ref())
			.and_then(StatusEmoji::from_activity);
		drop(state);
		let changed = latency != self.latency
			|| name != self.name
			|| handle != self.handle
			|| avatar != self.avatar
			|| status != self.status
			|| custom_status != self.custom_status
			|| custom_emoji != self.custom_emoji;
		if changed {
			self.latency = latency;
			self.name = name;
			self.handle = handle;
			self.avatar = avatar;
			self.status = status;
			self.custom_status = custom_status;
			self.custom_emoji = custom_emoji;
			cx.notify();
		}
	}
}

impl Render for UserPanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let hide_handle = streamer_mode(cx).hides_personal();
		let subtitle = self
			.custom_status
			.clone()
			.or_else(|| (!hide_handle).then(|| self.handle.clone()));
		let subtitle_emoji = self.custom_emoji.as_ref().map(|emoji| emoji.element(cx));
		let (muted, deafened, push_to_talk, talking) = {
			let data = self.data.read(cx);
			(
				data.self_mute(),
				data.self_deaf(),
				data.push_to_talk(),
				data.talking(),
			)
		};
		let mic = if muted || deafened {
			IconKind::MicOff
		} else {
			IconKind::Mic
		};
		let headphones = if deafened {
			IconKind::HeadphonesOff
		} else {
			IconKind::Headphones
		};
		let mic_tooltip = if muted {
			cx.t("user-unmute")
		} else {
			cx.t("user-mute")
		};
		let deafen_tooltip = if deafened {
			cx.t("user-undeafen")
		} else {
			cx.t("user-deafen")
		};
		let latency = match self.latency {
			Some(latency) => cx.t_args(
				"user-latency",
				&[("ms", ArgValue::from(latency.as_millis() as usize))],
			),
			None => cx.t("user-connecting"),
		};
		let menu = self.menu_element(&theme, cx);
		let menu_closed = menu.is_none();
		let danger = theme.danger;
		let mic_button = IconButton::new("user-mic", mic)
			.filled_hover()
			.hit_size(rem(32.))
			.tooltip(mic_tooltip)
			.when(muted || deafened, |button| button.tinted(danger))
			.on_click(cx.listener(|this, _, _, cx| {
				this.data.update(cx, |data, cx| data.toggle_mute(cx));
			}));
		let deafen_button = IconButton::new("user-deafen", headphones)
			.filled_hover()
			.hit_size(rem(32.))
			.tooltip(deafen_tooltip)
			.when(deafened, |button| button.tinted(danger))
			.on_click(cx.listener(|this, _, _, cx| {
				this.data.update(cx, |data, cx| data.toggle_deafen(cx));
			}));
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap(theme.space_4)
			.h(theme.user_panel_height)
			.px(theme.space_8)
			.child(
				div()
					.id("user-area")
					.flex()
					.flex_1()
					.min_w_0()
					.items_center()
					.gap(theme.space_8)
					.h(rem(40.))
					.px(theme.space_4)
					.rounded(theme.radius_md)
					.cursor_pointer()
					.hover(|style| style.bg(theme.modifier_hover))
					.on_click(
						cx.listener(|this, event: &ClickEvent, _, cx| this.toggle_menu(event.position(), cx)),
					)
					.when(menu_closed, |area| area.tooltip(tooltip(latency)))
					.child(
						Avatar::new(self.name.clone())
							.size(rem(32.))
							.url(self.avatar.clone())
							.status(self.status)
							.cutout(theme.user_panel_background),
					)
					.child(
						div()
							.flex()
							.flex_col()
							.min_w_0()
							.child(
								div()
									.truncate()
									.text_size(theme.font_sm)
									.line_height(rem(18.))
									.font_weight(FontWeight::SEMIBOLD)
									.text_color(theme.header_primary)
									.child(self.name.clone()),
							)
							.child(
								div()
									.flex()
									.items_center()
									.gap(theme.space_4)
									.min_w_0()
									.text_size(theme.font_tiny)
									.line_height(rem(13.))
									.text_color(theme.text_muted)
									.children(subtitle_emoji)
									.children(subtitle.map(|text| div().truncate().child(text))),
							),
					),
			)
			.when(push_to_talk, |panel| {
				panel.child(
					div()
						.id("user-ptt")
						.flex_none()
						.px(theme.space_4)
						.rounded(theme.radius_sm)
						.text_size(theme.font_tiny)
						.line_height(theme.voice_icon_sm)
						.font_weight(FontWeight::BOLD)
						.when(talking, |pill| {
							pill.bg(theme.success).text_color(theme.text_on_brand)
						})
						.when(!talking, |pill| {
							pill.bg(theme.modifier_selected).text_color(theme.text_muted)
						})
						.tooltip(tooltip(cx.t("voice-ptt-indicator")))
						.child(cx.t("voice-ptt")),
				)
			})
			.child(mic_button)
			.child(deafen_button)
			.child(
				IconButton::new("user-settings", IconKind::Settings)
					.filled_hover()
					.hit_size(rem(32.))
					.tooltip(cx.t("user-settings"))
					.on_click(cx.listener(|_, _, _, cx| cx.emit(UserPanelEvent::OpenSettings))),
			)
			.children(menu)
	}
}
