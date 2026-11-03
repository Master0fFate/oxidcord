use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
	AnyElement, AnyView, AppContext as _, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement as _, Pixels, Point, Rems, Render,
	SharedString, Size, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task,
	Window, canvas, div, img, prelude::FluentBuilder as _, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::DropdownMenu as _;
use protocol::{ChannelType, Permissions, Snowflake};
use session::Command;
use state::{VOICE_EFFECT_LIFETIME, VoiceSession, VoiceStatus};

use super::actions;
use super::live::{LiveStream, PREVIEW_HINT, live_streams, viewer_stack};
use super::participants::{Participant, participants};
use super::picker;
use super::popout;
use super::quality::Preset;
use super::soundboard;
use super::stage::open_topic;
use super::status;
use super::video_sink::{follow_frames, video_image};
use super::viewer::StreamViewer;
use super::widgets::{ControlButton, live_badge, speaking_avatar, state_icons};
use crate::components::{ContextMenuBuilder, IconButton, StatusEmoji};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::images::Images;
use crate::shell::SettingsBackend;
use crate::shell::composer::{Composer, accept_files};
use crate::theme::{Theme, rem};

const TILE_ASPECT: f32 = 9. / 16.;
const AVATAR_PIXELS: u32 = 160;
const IDLE_REDRAW: Duration = Duration::from_millis(500);
const STAGE_SPEAKER_SHARE: f32 = 0.55;
const AUDIENCE_ITEM_WIDTH: f32 = 88.;
const REQUEST_LIST_MAX_HEIGHT: f32 = 168.;
const EFFECT_SIZE: f32 = 3.;

pub enum RoomEvent {
	UserMenu { user: Snowflake, at: Point<Pixels> },
	OpenSettings,
	Fullscreen(Snowflake),
}

pub struct VoiceRoom {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	timeline: AnyView,
	composer: Entity<Composer>,
	chat_open: bool,
	focused: Option<Snowflake>,
	viewer: Option<Entity<StreamViewer>>,
	camera_pending: bool,
	soundboard_open: bool,
	effect_seen: Option<Instant>,
	stage: Rc<Cell<Size<Pixels>>>,
	last_render: Cell<Instant>,
	status_cache: Option<SharedString>,
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
	me: Option<Snowflake>,
	audience: bool,
	hand_raised: bool,
	invited: bool,
	server_muted: bool,
	topic: Option<SharedString>,
	moderator: bool,
	can_speak: bool,
	streams: HashMap<Snowflake, LiveStream>,
	can_chat: bool,
	voice_status: Option<SharedString>,
	can_edit_status: bool,
	effects: HashMap<Snowflake, Option<StatusEmoji>>,
}

enum TileSize {
	Fixed(Rems, Rems),
	Fill,
}

fn snapshot(data: &Data, status_cache: &mut Option<SharedString>) -> Option<Snapshot> {
	let self_mute = data.self_mute();
	let self_deaf = data.self_deaf();
	let state = data.state();
	let selected = state.selected()?;
	let channel = selected.channel_id;
	let kind = state.channel(channel).map(|channel| channel.kind);
	let session = state.voice().filter(|voice| voice.channel_id == channel);
	let participants = participants(&state, selected.guild_id, channel, AVATAR_PIXELS);
	let permissions = state.my_permissions(channel);
	let voice_status = state.channel(channel).and_then(|found| found.voice_status());
	if voice_status != status_cache.as_deref() {
		*status_cache = voice_status.map(|status| SharedString::from(status.to_owned()));
	}
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
		me: state.current_user_id(),
		audience: participants.iter().any(|person| person.me && person.listener),
		hand_raised: participants
			.iter()
			.any(|person| person.me && person.hand_raised.is_some()),
		invited: session.is_some() && state.speak_invited(),
		server_muted: session.is_some() && state.server_muted(),
		topic: selected
			.guild_id
			.and_then(|guild| state.stage_instance(guild, channel))
			.map(|instance| instance.topic.clone().into()),
		moderator: permissions
			.contains(Permissions::MANAGE_CHANNELS | Permissions::MUTE_MEMBERS | Permissions::MOVE_MEMBERS),
		can_speak: permissions.contains(Permissions::MUTE_MEMBERS),
		participants,
		streams: live_streams(&state, selected.guild_id, channel),
		can_chat: permissions.contains(Permissions::VIEW_CHANNEL | Permissions::READ_MESSAGE_HISTORY),
		voice_status: status_cache.clone(),
		can_edit_status: kind == Some(ChannelType::GuildVoice) && state.can_set_voice_status(channel),
		effects: live_effects(session),
	})
}

fn live_effects(session: Option<&VoiceSession>) -> HashMap<Snowflake, Option<StatusEmoji>> {
	session
		.into_iter()
		.flat_map(|voice| voice.effects.iter())
		.filter(|(_, effect)| effect.is_live())
		.map(|(user, effect)| (*user, effect.emoji.as_ref().and_then(StatusEmoji::from_activity)))
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
		composer: Entity<Composer>,
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
				if changes.streams || changes.selection {
					this.request_previews(cx);
				}
			}
		});
		let local = cx.observe(&data, |_, _, cx| cx.notify());
		let frames = follow_frames(cx, |room, cx| {
			if room.last_render.get().elapsed() < IDLE_REDRAW {
				cx.notify();
			}
		});
		Self {
			data,
			backend,
			timeline,
			composer,
			chat_open: false,
			focused: None,
			viewer: None,
			camera_pending: false,
			soundboard_open: false,
			effect_seen: None,
			stage: Rc::new(Cell::new(size(px(0.), px(0.)))),
			last_render: Cell::new(Instant::now()),
			status_cache: None,
			_subscriptions: vec![changes, local],
			_frames: frames,
		}
	}

	pub fn open_chat(&mut self, cx: &mut Context<Self>) {
		self.chat_open = true;
		cx.notify();
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
		self.expire_effects(cx);
		cx.notify();
	}

	fn expire_effects(&mut self, cx: &mut Context<Self>) {
		let latest = self
			.data
			.read(cx)
			.state()
			.voice()
			.and_then(|voice| voice.effects.values().map(|effect| effect.sent_at).max());
		if latest.is_none() || latest == self.effect_seen {
			return;
		}
		self.effect_seen = latest;
		cx.spawn(async move |this, cx| {
			cx.background_executor().timer(VOICE_EFFECT_LIFETIME).await;
			this.update(cx, |_, cx| cx.notify()).ok();
		})
		.detach();
	}

	fn toggle_soundboard(&mut self, guild: Snowflake, cx: &mut Context<Self>) {
		self.soundboard_open = !self.soundboard_open;
		if self.soundboard_open {
			self.data
				.read(cx)
				.send(Command::LoadSoundboard { guild_id: guild });
		}
		cx.notify();
	}

	fn request_previews(&self, cx: &Context<Self>) {
		let data = self.data.read(cx);
		let state = data.state();
		let Some(channel) = state.selected().map(|selected| selected.channel_id) else {
			return;
		};
		if !state.my_permissions(channel).contains(Permissions::CONNECT) {
			return;
		}
		let me = state.current_user_id();
		for stream in state.streams().filter(|stream| {
			stream.channel_id == channel && stream.preview_url.is_none() && Some(stream.user_id) != me
		}) {
			data.send(Command::LoadStreamPreview {
				stream_key: stream.stream_key.clone(),
			});
		}
	}

	fn toggle_focus(&mut self, user: Snowflake, cx: &mut Context<Self>) {
		let focus = self.focused != Some(user);
		self.focused = focus.then_some(user);
		self.viewer = focus.then(|| {
			let data = self.data.clone();
			cx.new(|cx| StreamViewer::new(user, &data, cx))
		});
		cx.notify();
	}

	fn focus_controls(
		&self,
		user: Snowflake,
		has_video: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let button = |id: &'static str, icon: IconKind, label: &str| {
			IconButton::new((id, user.get()), icon)
				.hit_size(rem(28.))
				.icon_size(theme.voice_icon_sm)
				.color(theme.interactive_active)
				.tooltip(cx.t(label))
		};
		div()
			.absolute()
			.right(theme.space_8)
			.top(theme.space_8)
			.flex()
			.gap(theme.space_4)
			.p(theme.space_4)
			.rounded(theme.radius_sm)
			.bg(theme.overlay)
			.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
			.when(has_video, |controls| {
				controls
					.child(
						button("voice-popout", IconKind::AppWindow, "voice-popout")
							.on_click(cx.listener(move |this, _, _, cx| popout::open(&this.data, user, cx))),
					)
					.child(
						button("voice-fullscreen", IconKind::Expand, "voice-fullscreen")
							.on_click(cx.listener(move |_, _, _, cx| cx.emit(RoomEvent::Fullscreen(user)))),
					)
			})
			.child(
				button("voice-unfocus", IconKind::Shrink, "voice-unfocus")
					.on_click(cx.listener(move |this, _, _, cx| this.toggle_focus(user, cx))),
			)
			.into_any_element()
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
		let image = video_image(cx, user);
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
		let live_stream = snapshot.streams.get(&user);
		let stream_key = live_stream
			.map(|stream| &stream.key)
			.filter(|key| !person.me && snapshot.watching.as_ref() != Some(*key))
			.cloned();
		let watched_key = snapshot
			.watching
			.clone()
			.filter(|key| !person.me && live_stream.is_some_and(|stream| &stream.key == key));
		let live_own = person.me && snapshot.screen_on;
		let icons = state_icons(person, theme.voice_badge_icon, theme);
		let live_label = cx.t("voice-live");
		let viewers = live_stream.and_then(|stream| viewer_stack(stream, theme));
		let preview = live_stream
			.and_then(|stream| stream.preview.as_ref())
			.filter(|_| image.is_none())
			.and_then(|url| cx.global::<Images>().get_still(url, PREVIEW_HINT));
		let zoomable = self
			.viewer
			.clone()
			.filter(|viewer| focused && image.is_some() && viewer.read(cx).user() == user);
		let content = match (&image, preview) {
			(Some(_), _) if zoomable.is_some() => div().size_full().children(zoomable),
			(Some(image), _) => div()
				.size_full()
				.bg(theme.background_floating)
				.child(img(image.clone()).size_full().object_fit(ObjectFit::Contain)),
			(None, Some(preview)) => div()
				.size_full()
				.bg(theme.background_floating)
				.child(img(preview).size_full().object_fit(ObjectFit::Cover)),
			(None, None) => div()
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
			.when(person.live || viewers.is_some(), |tile| {
				tile.child(
					div()
						.absolute()
						.left(theme.space_8)
						.top(theme.space_8)
						.flex()
						.items_center()
						.gap(theme.space_4)
						.when(person.live, |row| {
							row.child(live_badge(live_label.clone(), theme))
						})
						.children(viewers),
				)
			})
			.when_some(snapshot.effects.get(&user), |tile, effect| {
				tile.child(
					div()
						.absolute()
						.inset_0()
						.flex()
						.items_center()
						.justify_center()
						.child(match effect {
							Some(emoji) => emoji.element(rem(EFFECT_SIZE), cx),
							None => Icon::new(IconKind::Soundboard)
								.size(rem(EFFECT_SIZE))
								.color(theme.interactive_active)
								.into_any_element(),
						}),
				)
			})
			.when(focused, |tile| {
				tile.child(self.focus_controls(user, image.is_some(), theme, cx))
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
							data.read(cx)
								.send(Command::SetStreamQuality(preset.quality(None)));
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
		let mut requests: Vec<&Participant> = audience
			.iter()
			.copied()
			.filter(|person| person.hand_raised.is_some() && snapshot.can_speak)
			.collect();
		requests.sort_by_key(|person| person.hand_raised);
		let speaker_area = size(area.width, area.height * STAGE_SPEAKER_SHARE);
		let audience_count = audience.len();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.size_full()
			.children(self.stage_header(snapshot, theme, cx))
			.children(self.invite_banner(snapshot, theme, cx))
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
					.when_some(
						snapshot.guild.filter(|_| !requests.is_empty()),
						|column, guild_id| {
							column
								.child(section_label(
									cx.t_args(
										"voice-stage-requests",
										&[("count", ArgValue::from(requests.len()))],
									),
									theme,
								))
								.child(self.request_list(&requests, guild_id, snapshot.channel, theme, cx))
						},
					)
					.child(section_label(
						cx.t_args(
							"voice-stage-audience",
							&[("count", ArgValue::from(audience_count))],
						),
						theme,
					))
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
									.when(person.hand_raised.is_some(), |item| {
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

	fn stage_header(&self, snapshot: &Snapshot, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
		if snapshot.topic.is_none() && !snapshot.moderator {
			return None;
		}
		let channel = snapshot.channel;
		let live = snapshot.topic.is_some();
		let current = snapshot.topic.as_ref().map(ToString::to_string);
		let topic = snapshot.topic.clone().unwrap_or_else(|| cx.t("voice-stage-idle"));
		Some(
			div()
				.flex()
				.flex_none()
				.items_center()
				.gap(theme.space_8)
				.child(
					Icon::new(IconKind::Stage)
						.size(theme.voice_icon_sm)
						.color(theme.interactive_normal),
				)
				.child(
					div()
						.flex_1()
						.min_w_0()
						.truncate()
						.text_size(theme.font_md)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(if live {
							theme.header_primary
						} else {
							theme.text_muted
						})
						.child(topic),
				)
				.when(snapshot.moderator && live, |header| {
					header
						.child(
							IconButton::new("voice-stage-edit", IconKind::Edit)
								.tooltip(cx.t("voice-stage-edit"))
								.on_click(move |_, window, cx| {
									open_topic(window, cx, channel, current.clone())
								}),
						)
						.child(
							Button::new("voice-stage-end")
								.danger()
								.label(cx.t("voice-stage-end"))
								.on_click(cx.listener(move |this, _, _, cx| {
									this.data.read(cx).send(Command::EndStage { channel_id: channel });
								})),
						)
				})
				.when(snapshot.moderator && !live, |header| {
					header.child(
						Button::new("voice-stage-start")
							.primary()
							.label(cx.t("voice-stage-start"))
							.on_click(move |_, window, cx| open_topic(window, cx, channel, None)),
					)
				})
				.into_any_element(),
		)
	}

	fn invite_banner(
		&self,
		snapshot: &Snapshot,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let guild_id = snapshot.guild.filter(|_| snapshot.invited)?;
		let channel_id = snapshot.channel;
		Some(
			div()
				.flex()
				.flex_none()
				.items_center()
				.justify_between()
				.gap(theme.space_8)
				.px(theme.space_12)
				.py(theme.space_8)
				.rounded(theme.radius_md)
				.bg(theme.background_secondary)
				.child(
					div()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(theme.header_primary)
						.child(cx.t("voice-stage-invited")),
				)
				.child(
					div()
						.flex()
						.items_center()
						.gap(theme.space_8)
						.child(
							Button::new("voice-invite-decline")
								.outline()
								.label(cx.t("voice-stage-decline"))
								.on_click(cx.listener(move |this, _, _, cx| {
									this.data.read(cx).send(Command::RequestToSpeak {
										guild_id,
										channel_id,
										request: false,
									});
								})),
						)
						.child(
							Button::new("voice-invite-accept")
								.primary()
								.label(cx.t("voice-stage-accept"))
								.on_click(cx.listener(move |this, _, _, cx| {
									this.data
										.read(cx)
										.send(Command::AcceptSpeakInvite { guild_id, channel_id });
								})),
						),
				)
				.into_any_element(),
		)
	}

	fn request_list(
		&self,
		requests: &[&Participant],
		guild_id: Snowflake,
		channel_id: Snowflake,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let decide = move |user_id: Snowflake, speaker: bool| Command::SetSpeaker {
			guild_id,
			channel_id,
			user_id,
			speaker,
		};
		div()
			.id("voice-requests")
			.flex()
			.flex_col()
			.flex_none()
			.max_h(rem(REQUEST_LIST_MAX_HEIGHT))
			.overflow_y_scroll()
			.children(requests.iter().map(|person| {
				let user = person.user;
				div()
					.id(("voice-request", user.get()))
					.flex()
					.items_center()
					.gap(theme.space_8)
					.h(theme.voice_row_height)
					.px(theme.space_8)
					.rounded(theme.radius_sm)
					.hover(|style| style.bg(theme.modifier_hover))
					.child(speaking_avatar(person, theme.voice_row_avatar, theme))
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_size(theme.font_sm)
							.text_color(theme.text_normal)
							.child(person.name.clone()),
					)
					.child(
						IconButton::new(("voice-request-accept", user.get()), IconKind::Check)
							.color(theme.success)
							.tooltip(cx.t("voice-stage-accept"))
							.on_click(cx.listener(move |this, _, _, cx| {
								this.data.read(cx).send(decide(user, true));
							})),
					)
					.child(
						IconButton::new(("voice-request-dismiss", user.get()), IconKind::Close)
							.tooltip(cx.t("voice-stage-dismiss"))
							.on_click(cx.listener(move |this, _, _, cx| {
								this.data.read(cx).send(decide(user, false));
							})),
					)
			}))
			.into_any_element()
	}

	fn stage_seat_button(
		&self,
		snapshot: &Snapshot,
		guild_id: Snowflake,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let channel_id = snapshot.channel;
		let me = snapshot.me;
		let data = self.data.clone();
		let seat = move |speaker: bool| {
			let data = data.clone();
			move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
				if let Some(user_id) = me {
					data.read(cx).send(Command::SetSpeaker {
						guild_id,
						channel_id,
						user_id,
						speaker,
					});
				}
			}
		};
		if !snapshot.audience {
			return ControlButton::new("voice-bar-audience", IconKind::Members, cx.t("mod-move-audience"))
				.on_click(seat(false))
				.into_any_element();
		}
		if snapshot.can_speak {
			return ControlButton::new("voice-bar-speak", IconKind::Stage, cx.t("voice-stage-speak"))
				.on_click(seat(true))
				.into_any_element();
		}
		let request = !snapshot.hand_raised;
		let data = self.data.clone();
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
		})
		.into_any_element()
	}

	fn control_bar(&self, snapshot: &Snapshot, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let data = self.data.clone();
		let backend = self.backend.clone();
		let preset = Preset::from_index((self.backend.read)().voice.video_preset);
		let mic_off = snapshot.self_mute || snapshot.self_deaf || snapshot.server_muted;
		let mic_label = cx.t(if snapshot.server_muted {
			"voice-server-muted"
		} else if snapshot.self_mute {
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
					if mic_off { IconKind::MicOff } else { IconKind::Mic },
					mic_label,
				)
				.alert(mic_off)
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
				|bar, guild_id| bar.child(self.stage_seat_button(snapshot, guild_id, cx)),
			)
			.when_some(
				snapshot.guild.filter(|_| !snapshot.stage_channel),
				|bar, guild_id| {
					bar.child(
						ControlButton::new(
							"voice-bar-soundboard",
							IconKind::Soundboard,
							cx.t("voice-soundboard"),
						)
						.highlight(self.soundboard_open)
						.on_click(cx.listener(move |this, _, _, cx| this.toggle_soundboard(guild_id, cx))),
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

	fn status_chip(&self, snapshot: &Snapshot, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let channel = snapshot.channel;
		div()
			.absolute()
			.top(theme.space_8)
			.left(theme.space_8)
			.max_w(rem(24.))
			.flex()
			.items_center()
			.gap(theme.space_4)
			.pl(theme.space_8)
			.rounded(theme.radius_sm)
			.bg(theme.overlay)
			.text_size(theme.font_xs)
			.font_weight(FontWeight::MEDIUM)
			.text_color(theme.interactive_active)
			.children(
				snapshot
					.voice_status
					.clone()
					.map(|status| div().min_w_0().truncate().child(status)),
			)
			.when(snapshot.can_edit_status, |chip| {
				chip.child(
					IconButton::new("voice-status-edit", IconKind::Edit)
						.filled_hover()
						.tooltip(cx.t("voice-status-edit"))
						.on_click(move |_, window, cx| status::open(window, cx, channel)),
				)
			})
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
		accept_files(div(), self.composer.clone())
			.flex()
			.flex_col()
			.flex_none()
			.w(theme.voice_chat_width)
			.h_full()
			.bg(theme.background_primary)
			.border_l_1()
			.border_color(theme.divider)
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

fn section_label(text: SharedString, theme: &Theme) -> impl IntoElement {
	div()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::BOLD)
		.text_color(theme.header_secondary)
		.child(text.to_uppercase())
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
		let Some(snapshot) = snapshot(self.data.read(cx), &mut self.status_cache) else {
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
							.when(
								snapshot.voice_status.is_some() || snapshot.can_edit_status,
								|stage| stage.child(self.status_chip(&snapshot, &theme, cx)),
							)
							.when_some(
								snapshot
									.guild
									.filter(|_| connected && self.soundboard_open && !snapshot.stage_channel),
								|stage, guild| {
									stage.child(soundboard::panel(
										&self.data,
										guild,
										snapshot.channel,
										&theme,
										cx,
									))
								},
							)
							.when(snapshot.can_chat, |stage| {
								stage.child(
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
								)
							}),
					)
					.child(bar),
			)
			.when(self.chat_open && snapshot.can_chat, |room| {
				room.child(self.chat_panel(&theme))
			})
			.into_any_element()
	}
}
