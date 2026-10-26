use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
	AnyElement, AnyView, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _, IntoElement,
	MouseButton, MouseDownEvent, ObjectFit, ParentElement as _, Pixels, Point, Rems, Render, Size,
	StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task, Window, canvas, div,
	img, prelude::FluentBuilder as _, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::DropdownMenu as _;
use protocol::{ChannelType, Snowflake};
use session::Command;
use state::{State, VideoKind, VoiceStatus};

use super::actions;
use super::participants::{Participant, participants};
use super::picker;
use super::quality::Preset;
use super::video_sink::Textures;
use super::widgets::{ControlButton, live_badge, speaking_avatar, state_icons};
use crate::components::{ContextMenuBuilder, IconButton};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::SettingsBackend;
use crate::theme::{Theme, rem};

const TILE_ASPECT: f32 = 9. / 16.;
const AVATAR_PIXELS: u32 = 160;
const IDLE_REDRAW: Duration = Duration::from_millis(500);
const STAGE_SPEAKER_SHARE: f32 = 0.55;
const AUDIENCE_ITEM_WIDTH: f32 = 88.;

pub enum RoomEvent {
	UserMenu { user: Snowflake, at: Point<Pixels> },
	OpenSettings,
}

pub struct VoiceRoom {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	timeline: AnyView,
	composer: AnyView,
	chat_open: bool,
	focused: Option<Snowflake>,
	camera_pending: bool,
	stage: Rc<Cell<Size<Pixels>>>,
	last_render: Cell<Instant>,
	_subscriptions: Vec<Subscription>,
	_frames: Task<()>,
}

impl EventEmitter<RoomEvent> for VoiceRoom {}

struct Snapshot {
	guild: Option<Snowflake>,
	channel: Snowflake,
	stage_channel: bool,
	status: Option<VoiceStatus>,
	participants: Vec<Participant>,
	self_mute: bool,
	self_deaf: bool,
	camera_on: bool,
	screen_on: bool,
	watching: Option<String>,
	hand_raised: bool,
	streams: HashMap<Snowflake, String>,
}

enum TileSize {
	Fixed(Rems, Rems),
	Fill,
}

fn snapshot(data: &Data) -> Option<Snapshot> {
	let self_mute = data.self_mute();
	let self_deaf = data.self_deaf();
	let state = data.state();
	let selected = state.selected()?;
	let channel = selected.channel_id;
	let kind = state.channel(channel).map(|channel| channel.kind);
	let session = state.voice().filter(|voice| voice.channel_id == channel);
	let participants = participants(&state, selected.guild_id, channel, AVATAR_PIXELS);
	Some(Snapshot {
		guild: selected.guild_id,
		channel,
		stage_channel: kind == Some(ChannelType::GuildStageVoice),
		status: session.map(|voice| voice.status.clone()),
		self_mute,
		self_deaf,
		camera_on: session.is_some_and(|voice| voice.camera_on),
		screen_on: session.is_some_and(|voice| voice.screen_share.is_some()),
		watching: session.and_then(|voice| voice.watching.clone()),
		hand_raised: participants.iter().any(|person| person.me && person.hand_raised),
		participants,
		streams: stream_keys(&state, channel),
	})
}

fn stream_keys(state: &State, channel: Snowflake) -> HashMap<Snowflake, String> {
	state
		.streams()
		.filter(|stream| stream.channel_id == channel)
		.map(|stream| (stream.user_id, stream.stream_key.clone()))
		.collect()
}

fn grid_layout(count: usize, width: Pixels, height: Pixels, gap: Pixels, rem_size: Pixels) -> (Rems, Rems) {
	let count = count.max(1);
	let mut best: Option<f32> = None;
	for columns in 1..=count {
		let rows = count.div_ceil(columns);
		let tile_width = (f32::from(width) - f32::from(gap) * (columns as f32 - 1.)) / columns as f32;
		let tile_height = tile_width * TILE_ASPECT;
		let total = tile_height * rows as f32 + f32::from(gap) * (rows as f32 - 1.);
		if total <= f32::from(height) && best.is_none_or(|current| tile_width > current) {
			best = Some(tile_width);
		}
	}
	let tile_width = best.unwrap_or_else(|| {
		let columns = (count as f32).sqrt().ceil();
		(f32::from(width) - f32::from(gap) * (columns - 1.)) / columns
	});
	let rem_size = f32::from(rem_size);
	(
		Rems(tile_width.max(1.) / rem_size),
		Rems((tile_width * TILE_ASPECT).max(1.) / rem_size),
	)
}

impl VoiceRoom {
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		timeline: AnyView,
		composer: AnyView,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let changes = cx.subscribe_in(&data, window, |this, _, event: &DataEvent, _, cx| {
			if let DataEvent::Changed(changes) = event
				&& (changes.voice_session
					|| changes.streams
					|| changes.calls
					|| !changes.voice.is_empty()
					|| changes.selection)
			{
				this.on_voice_change(cx);
			}
		});
		let local = cx.observe(&data, |_, _, cx| cx.notify());
		let frames = match cx.try_global::<Textures>().cloned() {
			Some(textures) => cx.spawn(async move |this, cx| {
				loop {
					textures.0.changed().await;
					let retired = textures.0.take_retired();
					let alive = this
						.update(cx, |room, cx| {
							for image in retired {
								cx.drop_image(image, None);
							}
							if room.last_render.get().elapsed() < IDLE_REDRAW {
								cx.notify();
							}
						})
						.is_ok();
					if !alive {
						break;
					}
				}
			}),
			None => Task::ready(()),
		};
		Self {
			data,
			backend,
			timeline,
			composer,
			chat_open: false,
			focused: None,
			camera_pending: false,
			stage: Rc::new(Cell::new(size(px(0.), px(0.)))),
			last_render: Cell::new(Instant::now()),
			_subscriptions: vec![changes, local],
			_frames: frames,
		}
	}

	pub fn camera_when_connected(&mut self) {
		self.camera_pending = true;
	}

	fn on_voice_change(&mut self, cx: &mut Context<Self>) {
		if self.camera_pending {
			let connected = {
				let data = self.data.read(cx);
				let state = data.state();
				state.voice().is_some_and(|voice| {
					voice.status == VoiceStatus::Connected
						&& Some(voice.channel_id) == state.selected().map(|s| s.channel_id)
				})
			};
			if connected {
				self.camera_pending = false;
				let backend = self.backend.clone();
				actions::toggle_camera(&self.data, &backend, cx);
			}
		}
		cx.notify();
	}

	fn toggle_focus(&mut self, user: Snowflake, cx: &mut Context<Self>) {
		self.focused = if self.focused == Some(user) {
			None
		} else {
			Some(user)
		};
		cx.notify();
	}

	fn stage_size(&self, window: &Window, theme: &Theme) -> Size<Pixels> {
		let measured = self.stage.get();
		let pad = theme.pixels(theme.space_16) * 2.;
		if measured.width > px(0.) && measured.height > px(0.) {
			return size(measured.width - pad, measured.height - pad);
		}
		let viewport = window.viewport_size();
		size(viewport.width * 0.5, viewport.height * 0.6)
	}

	fn tile(
		&self,
		person: &Participant,
		tile_size: TileSize,
		compact: bool,
		snapshot: &Snapshot,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let user = person.user;
		let image = cx.try_global::<Textures>().and_then(|textures| {
			textures
				.0
				.image((user, VideoKind::Screen))
				.or_else(|| textures.0.image((user, VideoKind::Camera)))
		});
		let focused = self.focused == Some(user);
		let border = if person.speaking {
			theme.speaking_ring
		} else {
			theme.background_secondary
		};
		let avatar_size = if compact {
			theme.voice_tile_avatar * 0.6
		} else {
			theme.voice_tile_avatar
		};
		let stream_key = snapshot
			.streams
			.get(&user)
			.filter(|key| !person.me && snapshot.watching.as_ref() != Some(*key))
			.cloned();
		let watched_key = snapshot
			.watching
			.clone()
			.filter(|key| !person.me && snapshot.streams.get(&user) == Some(key));
		let live_own = person.me && snapshot.screen_on;
		let icons = state_icons(person, theme.voice_badge_icon, theme);
		let live_label = cx.t("voice-live");
		let content = match &image {
			Some(image) => div()
				.size_full()
				.bg(theme.background_floating)
				.child(img(image.clone()).size_full().object_fit(ObjectFit::Contain)),
			None => div()
				.size_full()
				.flex()
				.items_center()
				.justify_center()
				.child(speaking_avatar(person, avatar_size, theme)),
		};
		let tile = div()
			.id(("voice-tile", user.get()))
			.relative()
			.flex_none()
			.overflow_hidden()
			.rounded(theme.radius_md)
			.border_2()
			.border_color(border)
			.bg(theme.background_secondary)
			.cursor_pointer()
			.on_click(cx.listener(move |this, _, _, cx| this.toggle_focus(user, cx)))
			.on_mouse_down(
				MouseButton::Right,
				cx.listener(move |_, event: &MouseDownEvent, _, cx| {
					cx.emit(RoomEvent::UserMenu {
						user,
						at: event.position,
					})
				}),
			)
			.child(content)
			.child(
				div()
					.absolute()
					.left(theme.space_8)
					.bottom(theme.space_8)
					.max_w_full()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.px(theme.space_8)
					.py(theme.space_4)
					.rounded(theme.radius_sm)
					.bg(theme.overlay)
					.text_size(theme.font_xs)
					.font_weight(FontWeight::MEDIUM)
					.text_color(theme.interactive_active)
					.child(div().truncate().child(person.name.clone()))
					.children(icons),
			)
			.when(person.live, |tile| {
				tile.child(
					div()
						.absolute()
						.left(theme.space_8)
						.top(theme.space_8)
						.child(live_badge(live_label.clone(), theme)),
				)
			})
			.when(focused, |tile| {
				tile.child(
					div().absolute().right(theme.space_8).top(theme.space_8).child(
						Icon::new(IconKind::Shrink)
							.size(theme.voice_icon_sm)
							.color(theme.interactive_active),
					),
				)
			})
			.when_some(watched_key, |tile, key| {
				tile.child(
					div().absolute().right(theme.space_8).bottom(theme.space_8).child(
						Button::new(("voice-unwatch", user.get()))
							.outline()
							.label(cx.t("voice-stop-watching"))
							.on_click(cx.listener(move |this, _, _, cx| {
								this.data.read(cx).send(Command::StopWatchingStream {
									stream_key: key.clone(),
								});
							})),
					),
				)
			})
			.when(live_own, |tile| {
				let menu = Preset::ALL
					.iter()
					.fold(ContextMenuBuilder::new(), |menu, preset| {
						let data = self.data.clone();
						let preset = *preset;
						menu.item(cx.t(preset.key()), None, move |_, cx| {
							data.read(cx).send(Command::SetVideoQuality(preset.quality(None)));
						})
					})
					.build();
				tile.child(
					div().absolute().right(theme.space_8).bottom(theme.space_8).child(
						Button::new("voice-live-quality")
							.outline()
							.label(cx.t("voice-picker-quality"))
							.dropdown_caret(true)
							.dropdown_menu(menu),
					),
				)
			})
			.when_some(stream_key, |tile, key| {
				tile.child(
					div()
						.absolute()
						.inset_0()
						.flex()
						.items_center()
						.justify_center()
						.child(
							Button::new(("voice-watch", user.get()))
								.primary()
								.label(cx.t("voice-watch-stream"))
								.on_click(cx.listener(move |this, _, _, cx| {
									this.data.read(cx).send(Command::WatchStream {
										stream_key: key.clone(),
									});
								})),
						),
				)
			});
		match tile_size {
			TileSize::Fixed(width, height) => tile.w(width).h(height).into_any_element(),
			TileSize::Fill => tile.flex_1().min_w_0().min_h_0().w_full().into_any_element(),
		}
	}

	fn grid(
		&self,
		people: &[&Participant],
		area: Size<Pixels>,
		snapshot: &Snapshot,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let (width, height) = grid_layout(
			people.len(),
			area.width,
			area.height,
			theme.pixels(theme.space_8),
			theme.pixels(Rems(1.)),
		);
		div()
			.flex()
			.flex_wrap()
			.justify_center()
			.content_center()
			.items_center()
			.gap(theme.space_8)
			.size_full()
			.children(
				people.iter().map(|person| {
					self.tile(person, TileSize::Fixed(width, height), false, snapshot, theme, cx)
				}),
			)
			.into_any_element()
	}

	fn focus_layout(
		&self,
		focused: &Participant,
		area: Size<Pixels>,
		snapshot: &Snapshot,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let strip_width = theme.voice_strip_tile;
		let strip_height = strip_width * TILE_ASPECT;
		let others: Vec<AnyElement> = snapshot
			.participants
			.iter()
			.filter(|person| person.user != focused.user)
			.map(|person| {
				self.tile(
					person,
					TileSize::Fixed(strip_width, strip_height),
					true,
					snapshot,
					theme,
					cx,
				)
			})
			.collect();
		let has_strip = !others.is_empty();
		let _ = area;
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.size_full()
			.child(div().flex().flex_1().min_h_0().child(self.tile(
				focused,
				TileSize::Fill,
				false,
				snapshot,
				theme,
				cx,
			)))
			.when(has_strip, |layout| {
				layout.child(
					div()
						.flex()
						.flex_none()
						.justify_center()
						.gap(theme.space_8)
						.children(others),
				)
			})
			.into_any_element()
	}

	fn stage_layout(
		&self,
		area: Size<Pixels>,
		snapshot: &Snapshot,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let speakers: Vec<&Participant> = snapshot.participants.iter().filter(|p| !p.listener).collect();
		let audience: Vec<&Participant> = snapshot.participants.iter().filter(|p| p.listener).collect();
		let speaker_area = size(area.width, area.height * STAGE_SPEAKER_SHARE);
		let audience_count = audience.len();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.size_full()
			.child(div().flex_none().h(speaker_area.height).child(self.grid(
				&speakers,
				speaker_area,
				snapshot,
				theme,
				cx,
			)))
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_h_0()
					.gap(theme.space_8)
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(
								cx.t_args(
									"voice-stage-audience",
									&[("count", ArgValue::from(audience_count))],
								)
								.to_uppercase(),
							),
					)
					.child(
						div()
							.id("voice-audience")
							.flex()
							.flex_wrap()
							.content_start()
							.gap(theme.space_8)
							.overflow_y_scroll()
							.children(audience.iter().map(|person| {
								let user = person.user;
								div()
									.id(("voice-audience-member", user.get()))
									.relative()
									.flex()
									.flex_col()
									.items_center()
									.gap(theme.space_4)
									.w(rem(AUDIENCE_ITEM_WIDTH))
									.on_mouse_down(
										MouseButton::Right,
										cx.listener(move |_, event: &MouseDownEvent, _, cx| {
											cx.emit(RoomEvent::UserMenu {
												user,
												at: event.position,
											})
										}),
									)
									.child(speaking_avatar(person, theme.voice_control, theme))
									.when(person.hand_raised, |item| {
										item.child(
											div().absolute().top_0().right(theme.space_12).child(
												Icon::new(IconKind::Hand)
													.size(theme.voice_icon_sm)
													.color(theme.warning),
											),
										)
									})
									.child(
										div()
											.w_full()
											.truncate()
											.text_center()
											.text_size(theme.font_tiny)
											.text_color(theme.text_muted)
											.child(person.name.clone()),
									)
							})),
					),
			)
			.into_any_element()
	}

	fn control_bar(&self, snapshot: &Snapshot, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let data = self.data.clone();
		let backend = self.backend.clone();
		let preset = Preset::from_index((self.backend.read)().voice.video_preset);
		let mic_label = cx.t(if snapshot.self_mute {
			"user-unmute"
		} else {
			"user-mute"
		});
		let deaf_label = cx.t(if snapshot.self_deaf {
			"user-undeafen"
		} else {
			"user-deafen"
		});
		let camera_label = cx.t(if snapshot.camera_on {
			"voice-camera-off"
		} else {
			"voice-camera-on"
		});
		let screen_label = cx.t(if snapshot.screen_on {
			"voice-screen-stop"
		} else {
			"voice-screen-share"
		});
		div()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.gap(theme.space_12)
			.py(theme.space_16)
			.child(
				ControlButton::new(
					"voice-bar-mic",
					if snapshot.self_mute || snapshot.self_deaf {
						IconKind::MicOff
					} else {
						IconKind::Mic
					},
					mic_label,
				)
				.alert(snapshot.self_mute || snapshot.self_deaf)
				.on_click({
					let data = data.clone();
					move |_, _, cx| data.update(cx, |data, cx| data.toggle_mute(cx))
				}),
			)
			.child(
				ControlButton::new(
					"voice-bar-deafen",
					if snapshot.self_deaf {
						IconKind::HeadphonesOff
					} else {
						IconKind::Headphones
					},
					deaf_label,
				)
				.alert(snapshot.self_deaf)
				.on_click({
					let data = data.clone();
					move |_, _, cx| data.update(cx, |data, cx| data.toggle_deafen(cx))
				}),
			)
			.child(
				ControlButton::new(
					"voice-bar-camera",
					if snapshot.camera_on {
						IconKind::Video
					} else {
						IconKind::VideoOff
					},
					camera_label,
				)
				.highlight(snapshot.camera_on)
				.on_click({
					let data = data.clone();
					move |_, _, cx| actions::toggle_camera(&data, &backend, cx)
				}),
			)
			.child(
				ControlButton::new(
					"voice-bar-screen",
					if snapshot.screen_on {
						IconKind::ScreenShareOff
					} else {
						IconKind::ScreenShare
					},
					screen_label,
				)
				.highlight(snapshot.screen_on)
				.on_click({
					let data = data.clone();
					move |_, window, cx| {
						if snapshot_screen_on(&data, cx) {
							actions::stop_screen_share(&data, cx);
						} else {
							picker::open(&data, preset, window, cx);
						}
					}
				}),
			)
			.when_some(
				snapshot.guild.filter(|_| snapshot.stage_channel),
				|bar, guild_id| {
					let channel_id = snapshot.channel;
					let request = !snapshot.hand_raised;
					let data = data.clone();
					bar.child(
						ControlButton::new(
							"voice-bar-hand",
							IconKind::Hand,
							cx.t(if request {
								"voice-stage-request"
							} else {
								"voice-stage-cancel-request"
							}),
						)
						.highlight(snapshot.hand_raised)
						.on_click(move |_, _, cx| {
							data.read(cx).send(Command::RequestToSpeak {
								guild_id,
								channel_id,
								request,
							})
						}),
					)
				},
			)
			.child(
				ControlButton::new(
					"voice-bar-disconnect",
					IconKind::PhoneOff,
					cx.t("voice-disconnect"),
				)
				.danger()
				.on_click({
					let data = data.clone();
					move |_, _, cx| actions::leave(&data, cx)
				}),
			)
			.child(
				ControlButton::new("voice-bar-settings", IconKind::Settings, cx.t("user-settings"))
					.on_click(cx.listener(|_, _, _, cx| cx.emit(RoomEvent::OpenSettings))),
			)
			.into_any_element()
	}

	fn join_prompt(&self, snapshot: &Snapshot, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let data = self.data.clone();
		let (guild, channel) = (snapshot.guild, snapshot.channel);
		let connecting = matches!(snapshot.status, Some(VoiceStatus::Connecting));
		div()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.py(theme.space_16)
			.child(
				Button::new("voice-join")
					.primary()
					.loading(connecting)
					.label(cx.t(if snapshot.stage_channel {
						"voice-join-stage"
					} else {
						"voice-join"
					}))
					.on_click(move |_, _, cx| actions::join(&data, guild, channel, cx)),
			)
			.into_any_element()
	}

	fn chat_panel(&self, theme: &Theme) -> impl IntoElement {
		div()
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.voice_chat_width)
			.h_full()
			.bg(theme.background_primary)
			.border_l_1()
			.border_color(theme.background_tertiary)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_h_0()
					.child(self.timeline.clone()),
			)
			.child(self.composer.clone())
	}
}

fn snapshot_screen_on(data: &Entity<Data>, cx: &gpui::App) -> bool {
	data.read(cx)
		.state()
		.voice()
		.is_some_and(|voice| voice.screen_share.is_some())
}

impl Render for VoiceRoom {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.last_render.set(Instant::now());
		let theme = Theme::current(cx);
		let Some(snapshot) = snapshot(self.data.read(cx)) else {
			return div().flex_1().into_any_element();
		};
		let area = self.stage_size(window, &theme);
		let connected = snapshot.status.is_some();
		let people: Vec<&Participant> = snapshot.participants.iter().collect();
		let focused = self
			.focused
			.and_then(|user| snapshot.participants.iter().find(|person| person.user == user));
		let content = if snapshot.stage_channel {
			self.stage_layout(area, &snapshot, &theme, cx)
		} else if people.is_empty() {
			div()
				.size_full()
				.flex()
				.items_center()
				.justify_center()
				.text_size(theme.font_md)
				.text_color(theme.text_muted)
				.child(cx.t("voice-empty"))
				.into_any_element()
		} else if let Some(focused) = focused {
			self.focus_layout(focused, area, &snapshot, &theme, cx)
		} else {
			self.grid(&people, area, &snapshot, &theme, cx)
		};
		let measure = {
			let cell = self.stage.clone();
			let entity = cx.entity();
			canvas(
				move |bounds, _, cx| {
					if cell.get() != bounds.size {
						cell.set(bounds.size);
						entity.update(cx, |_, cx| cx.notify());
					}
				},
				|_, _, _, _| {},
			)
			.absolute()
			.size_full()
		};
		let bar = if connected {
			self.control_bar(&snapshot, &theme, cx)
		} else {
			self.join_prompt(&snapshot, &theme, cx)
		};
		let chat_label = cx.t(if self.chat_open {
			"voice-chat-hide"
		} else {
			"voice-chat-show"
		});
		div()
			.flex()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.h_full()
					.bg(theme.background_tertiary)
					.child(
						div()
							.relative()
							.flex_1()
							.min_h_0()
							.p(theme.space_16)
							.child(measure)
							.child(content)
							.child(
								div().absolute().top(theme.space_8).right(theme.space_8).child(
									IconButton::new("voice-chat-toggle", IconKind::Chat)
										.filled_hover()
										.active(self.chat_open)
										.tooltip(chat_label)
										.on_click(cx.listener(|this, _, _, cx| {
											this.chat_open = !this.chat_open;
											cx.notify();
										})),
								),
							),
					)
					.child(bar),
			)
			.when(self.chat_open, |room| room.child(self.chat_panel(&theme)))
			.into_any_element()
	}
}
