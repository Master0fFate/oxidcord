use std::rc::Rc;

use gpui::{
	Context, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
	SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use gpui_component::popover::Popover;
use protocol::Snowflake;
use state::{State, VoiceStats, VoiceStatus};

use super::actions;
use super::picker;
use super::quality::Preset;
use super::video_sink::Textures;
use super::widgets::{SignalTrigger, signal};
use crate::components::{IconButton, tooltip};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::{SettingsBackend, plugins};
use crate::theme::Theme;

pub struct VoicePanel {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	title: SharedString,
	titled: Option<(Option<Snowflake>, Snowflake)>,
	last_error: Option<String>,
	was_camera: bool,
	was_screen: bool,
	_subscriptions: Vec<Subscription>,
}

pub fn channel_title(state: &State, guild: Option<Snowflake>, channel: Snowflake) -> SharedString {
	match guild {
		Some(guild_id) => {
			let name = state
				.channel(channel)
				.and_then(|channel| channel.name.clone())
				.unwrap_or_default();
			match state.guild(guild_id) {
				Some(guild) => format!("{name} / {}", guild.name).into(),
				None => name.into(),
			}
		}
		None => state
			.dm_rows()
			.into_iter()
			.find(|row| row.channel_id == channel)
			.map(|row| row.name)
			.unwrap_or_default()
			.into(),
	}
}

impl VoicePanel {
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let subscription = cx.subscribe_in(&data, window, |this, _, event: &DataEvent, window, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.voice_session
					|| changes.streams
					|| changes.private_channels
					|| changes.guild_list)
			{
				this.on_voice_change(window, cx);
			}
		});
		let local = cx.observe(&data, |_, _, cx| cx.notify());
		Self {
			data,
			backend,
			title: SharedString::default(),
			titled: None,
			last_error: None,
			was_camera: false,
			was_screen: false,
			_subscriptions: vec![subscription, local],
		}
	}

	fn on_voice_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let (target, error, camera, screen, me) = {
			let data = self.data.read(cx);
			let state = data.state();
			let me = state.current_user_id();
			match state.voice() {
				Some(voice) => {
					let error = match &voice.status {
						VoiceStatus::Disconnected { reason } => reason.clone(),
						_ => None,
					};
					(
						Some((voice.guild_id, voice.channel_id)),
						error,
						voice.camera_on,
						voice.screen_share.is_some(),
						me,
					)
				}
				None => (None, None, false, false, me),
			}
		};
		if target != self.titled {
			self.title = target.map_or_else(SharedString::default, |(guild, channel)| {
				channel_title(&self.data.read(cx).state(), guild, channel)
			});
			self.titled = target;
		}
		if let Some(reason) = error
			.as_ref()
			.filter(|reason| self.last_error.as_ref() != Some(*reason))
		{
			self.last_error = Some(reason.clone());
			window.push_notification(
				Notification::error(reason.clone()).title(cx.t("voice-not-connected")),
				cx,
			);
		} else if target.is_none() || error.is_none() {
			self.last_error = None;
		}
		self.release_previews(camera, screen, target.is_none(), me, cx);
		self.was_camera = camera;
		self.was_screen = screen;
		cx.notify();
	}

	fn release_previews(
		&self,
		camera: bool,
		screen: bool,
		ended: bool,
		me: Option<Snowflake>,
		cx: &mut Context<Self>,
	) {
		let (Some(textures), Some(me)) = (cx.try_global::<Textures>().cloned(), me) else {
			return;
		};
		if self.was_camera && !camera || ended {
			textures.0.stop_preview((me, state::VideoKind::Camera));
		}
		if self.was_screen && !screen || ended {
			textures.0.stop_preview((me, state::VideoKind::Screen));
		}
	}

	fn stats_rows(&self, stats: VoiceStats, cx: &Context<Self>) -> Vec<(SharedString, SharedString)> {
		let ping = stats.ping_ms.map_or_else(
			|| cx.t("voice-stats-unknown"),
			|ms| cx.t_args("voice-ping", &[("ms", ArgValue::from(ms as usize))]),
		);
		let jitter = cx.t_args(
			"voice-ping",
			&[("ms", ArgValue::from(stats.jitter_ms.round() as usize))],
		);
		let mut rows = vec![
			(cx.t("voice-stats-ping"), ping),
			(cx.t("voice-stats-loss"), stats.loss.to_string().into()),
			(cx.t("voice-stats-jitter"), jitter),
		];
		if stats.video_bitrate_bps > 0 {
			rows.push((
				cx.t("voice-stats-bitrate"),
				cx.t_args(
					"voice-kbps",
					&[("kbps", ArgValue::from((stats.video_bitrate_bps / 1000) as usize))],
				),
			));
		}
		rows
	}

	fn action_button(
		&self,
		id: &'static str,
		icon: IconKind,
		active: bool,
		tooltip_text: SharedString,
		theme: &Theme,
	) -> gpui::Stateful<gpui::Div> {
		let (background, color) = if active {
			(theme.interactive_active, theme.background_tertiary)
		} else {
			(theme.modifier_selected, theme.interactive_normal)
		};
		div()
			.id(id)
			.flex()
			.flex_1()
			.items_center()
			.justify_center()
			.h(theme.voice_button)
			.rounded(theme.radius_sm)
			.bg(background)
			.cursor_pointer()
			.when(!active, |button| {
				button.hover(|style| style.bg(theme.modifier_active))
			})
			.tooltip(tooltip(tooltip_text))
			.child(Icon::new(icon).size(theme.voice_icon_md).color(color))
	}
}

fn status_style(status: &VoiceStatus, theme: &Theme) -> (&'static str, Hsla) {
	match status {
		VoiceStatus::Connecting => ("voice-connecting", theme.warning),
		VoiceStatus::Connected => ("voice-connected", theme.success),
		VoiceStatus::Reconnecting => ("voice-reconnecting", theme.warning),
		VoiceStatus::Disconnected { reason: Some(_) } => ("voice-not-connected", theme.danger),
		VoiceStatus::Disconnected { reason: None } => ("voice-disconnected", theme.text_muted),
	}
}

impl Render for VoicePanel {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let snapshot = {
			let data = self.data.read(cx);
			let state = data.state();
			state.voice().map(|voice| {
				(
					voice.status.clone(),
					voice.stats,
					voice.camera_on,
					voice.screen_share.is_some(),
					voice.channel_id,
					voice.guild_id,
				)
			})
		};
		let Some((status, stats, camera_on, screen_on, channel, guild)) = snapshot else {
			return div().into_any_element();
		};
		let (status_key, status_color) = status_style(&status, &theme);
		let failed = matches!(status, VoiceStatus::Disconnected { reason: Some(_) });
		let connected = matches!(status, VoiceStatus::Connected);
		let (signal_icon, signal_color) = if failed {
			(IconKind::Warning, theme.danger)
		} else {
			signal(stats.ping_ms.filter(|_| connected), &theme)
		};
		let ping_text = stats
			.ping_ms
			.filter(|_| connected)
			.map(|ms| cx.t_args("voice-ping", &[("ms", ArgValue::from(ms as usize))]));
		let rows = Rc::new(self.stats_rows(stats, cx));
		let stats_title = cx.t("voice-stats-title");
		let e2ee_text = cx.t(if stats.e2ee {
			"voice-e2ee-on"
		} else {
			"voice-e2ee-off"
		});
		let e2ee = stats.e2ee;
		let trigger = SignalTrigger::new(signal_icon, signal_color);
		let signal_button = if failed {
			div().child(trigger).into_any_element()
		} else {
			Popover::new("voice-stats")
				.trigger(trigger)
				.content(move |_, _, cx| {
					let theme = Theme::current(cx);
					div()
						.flex()
						.flex_col()
						.gap(theme.space_8)
						.w(theme.voice_menu_width)
						.child(
							div()
								.text_size(theme.font_sm)
								.font_weight(FontWeight::SEMIBOLD)
								.text_color(theme.header_primary)
								.child(stats_title.clone()),
						)
						.children(rows.iter().map(|(label, value)| {
							div()
								.flex()
								.items_center()
								.justify_between()
								.text_size(theme.font_sm)
								.child(div().text_color(theme.text_muted).child(label.clone()))
								.child(div().text_color(theme.text_normal).child(value.clone()))
						}))
						.child(
							div()
								.flex()
								.items_center()
								.gap(theme.space_4)
								.text_size(theme.font_sm)
								.text_color(if e2ee { theme.success } else { theme.text_muted })
								.child(
									Icon::new(IconKind::Lock)
										.size(theme.voice_icon_sm)
										.color(if e2ee { theme.success } else { theme.text_muted }),
								)
								.child(e2ee_text.clone()),
						)
				})
				.into_any_element()
		};
		let data = self.data.clone();
		let backend = self.backend.clone();
		let screen_backend = self.backend.clone();
		let camera_tooltip = cx.t(if camera_on {
			"voice-camera-off"
		} else {
			"voice-camera-on"
		});
		let screen_tooltip = cx.t(if screen_on {
			"voice-screen-stop"
		} else {
			"voice-screen-share"
		});
		let camera_button = self
			.action_button(
				"voice-camera",
				if camera_on {
					IconKind::Video
				} else {
					IconKind::VideoOff
				},
				camera_on,
				camera_tooltip,
				&theme,
			)
			.on_click({
				let data = data.clone();
				move |_, _, cx| actions::toggle_camera(&data, &backend, cx)
			});
		let screen_button = self
			.action_button(
				"voice-screen",
				if screen_on {
					IconKind::ScreenShareOff
				} else {
					IconKind::ScreenShare
				},
				screen_on,
				screen_tooltip,
				&theme,
			)
			.on_click({
				let data = data.clone();
				move |_, window, cx| {
					if screen_on {
						actions::stop_screen_share(&data, cx);
					} else {
						let preset = Preset::from_index((screen_backend.read)().voice.video_preset);
						picker::open(&data, preset, window, cx);
					}
				}
			});
		let retry = failed.then(|| {
			let data = data.clone();
			IconButton::new("voice-retry", IconKind::Join)
				.filled_hover()
				.hit_size(theme.voice_button)
				.icon_size(theme.voice_icon_md)
				.tooltip(cx.t("voice-retry"))
				.on_click(move |_, _, cx| actions::join(&data, guild, channel, cx))
		});
		let mounted = plugins::mount_column("voice-panel", cx);
		div()
			.flex()
			.flex_col()
			.flex_none()
			.gap(theme.space_8)
			.px(theme.space_8)
			.py(theme.space_8)
			.bg(theme.user_panel_background)
			.border_b_1()
			.border_color(theme.divider)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.child(signal_button)
					.child(
						div()
							.flex()
							.flex_col()
							.flex_1()
							.min_w_0()
							.child(
								div()
									.flex()
									.items_center()
									.gap(theme.space_4)
									.text_size(theme.font_sm)
									.font_weight(FontWeight::SEMIBOLD)
									.text_color(status_color)
									.child(div().truncate().child(cx.t(status_key)))
									.when(connected && stats.e2ee, |row| {
										row.child(
											div()
												.id("voice-e2ee")
												.flex_none()
												.tooltip(tooltip(cx.t("voice-e2ee-on")))
												.child(
													Icon::new(IconKind::Lock)
														.size(theme.voice_icon_sm)
														.color(theme.success),
												),
										)
									})
									.when_some(ping_text, |row, ping| {
										row.child(
											div()
												.flex_none()
												.font_weight(FontWeight::NORMAL)
												.text_size(theme.font_tiny)
												.text_color(theme.text_muted)
												.child(ping),
										)
									}),
							)
							.child(
								div()
									.truncate()
									.text_size(theme.font_tiny)
									.text_color(theme.text_muted)
									.child(self.title.clone()),
							),
					)
					.children(retry)
					.child({
						let data = data.clone();
						IconButton::new("voice-disconnect", IconKind::PhoneOff)
							.filled_hover()
							.hit_size(theme.voice_button)
							.icon_size(theme.voice_icon_md)
							.tooltip(cx.t("voice-disconnect"))
							.on_click(move |_, _, cx| actions::leave(&data, cx))
					}),
			)
			.when(!failed, |panel| {
				panel.child(
					div()
						.flex()
						.items_center()
						.gap(theme.space_8)
						.child(camera_button)
						.child(screen_button),
				)
			})
			.children(mounted)
			.into_any_element()
	}
}
