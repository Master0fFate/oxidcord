use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gpui::{
	AnyElement, App, Bounds, ClickEvent, DispatchPhase, ElementId, FontWeight, Hsla, InteractiveElement as _,
	IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement as _,
	Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window, canvas, div, hsla,
	prelude::FluentBuilder as _, relative,
};
use player::{State, Status};

use super::RowAction;
use super::body::BodyEnv;
use super::media::picture;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::playback::{MediaKey, Playback, hint_size};
use crate::theme::{Theme, rem};

const CENTER_BUTTON: f32 = 48.;
const CONTROL_ICON: f32 = 18.;
const CONTROL_HIT: f32 = 28.;
const CONTROLS_HEIGHT: f32 = 40.;
const HEADER_HEIGHT: f32 = 40.;
const BAR_HEIGHT: f32 = 4.;
const BAR_HIT: f32 = 16.;
const VOLUME_WIDTH: f32 = 56.;
const AUDIO_WIDTH: f32 = 432.;
const AUDIO_ICON: f32 = 32.;
const VOICE_WIDTH: f32 = 300.;
const VOICE_HEIGHT: f32 = 48.;
const VOICE_BUTTON: f32 = 32.;
const VOICE_BARS: usize = 44;
const VOICE_BAR_WIDTH: f32 = 3.;
const VOICE_BAR_MIN: f32 = 2.;
const VOICE_BAR_MAX: f32 = 24.;
const WAVEFORM_PEAK: f32 = 255.;

pub struct FileInfo {
	pub name: String,
	pub size: Option<String>,
	pub download: String,
}

pub struct VideoSource<'a> {
	pub key: MediaKey,
	pub url: String,
	pub poster: Option<String>,
	pub placeholder: Option<&'a str>,
	pub width: f32,
	pub height: f32,
	pub file: Option<FileInfo>,
}

pub struct LoopSource<'a> {
	pub key: MediaKey,
	pub url: String,
	pub poster: Option<String>,
	pub placeholder: Option<&'a str>,
	pub width: f32,
	pub height: f32,
}

pub struct AudioSource {
	pub key: MediaKey,
	pub url: String,
	pub file: FileInfo,
}

pub struct VoiceSource<'a> {
	pub key: MediaKey,
	pub url: String,
	pub waveform: Option<&'a str>,
	pub duration: Option<f64>,
}

fn white(alpha: f32) -> Hsla {
	hsla(0., 0., 1., alpha)
}

fn black(alpha: f32) -> Hsla {
	hsla(0., 0., 0., alpha)
}

fn clock(at: Duration) -> String {
	let total = at.as_secs();
	if total >= 3600 {
		format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
	} else {
		format!("{}:{:02}", total / 60, total % 60)
	}
}

fn share(status: Option<Status>, scrub: Option<f32>) -> f32 {
	if let Some(scrub) = scrub {
		return scrub;
	}
	match status.and_then(|status| status.duration.map(|duration| (status.position, duration))) {
		Some((position, duration)) if !duration.is_zero() => {
			(position.as_secs_f32() / duration.as_secs_f32()).clamp(0., 1.)
		}
		_ => 0.,
	}
}

fn shown_position(status: Option<Status>, scrub: Option<f32>) -> Duration {
	let Some(status) = status else {
		return Duration::ZERO;
	};
	match (scrub, status.duration) {
		(Some(scrub), Some(duration)) => duration.mul_f32(scrub),
		_ => status.position,
	}
}

fn group_name(prefix: &str, key: MediaKey) -> SharedString {
	format!("{prefix}-{}-{}", key.message, key.item).into()
}

fn element_id(prefix: &'static str, key: MediaKey) -> ElementId {
	ElementId::NamedInteger(prefix.into(), key.message ^ key.item.rotate_left(17))
}

fn toggler(key: MediaKey, url: String, max_size: (u32, u32)) -> impl Fn(&ClickEvent, &mut Window, &mut App) {
	move |_, window, cx| {
		cx.stop_propagation();
		cx.global::<Playback>().toggle(key, &url, max_size);
		window.refresh();
	}
}

fn play_icon(status: Option<Status>) -> IconKind {
	match status.map(|status| status.state) {
		Some(State::Playing | State::Loading) => IconKind::Pause,
		Some(State::Ended) => IconKind::Replay,
		_ => IconKind::Play,
	}
}

fn play_label(status: Option<Status>, cx: &App) -> SharedString {
	cx.t(match status.map(|status| status.state) {
		Some(State::Playing | State::Loading) => "media-pause",
		Some(State::Ended) => "media-replay",
		_ => "media-play",
	})
}

fn control(id: ElementId, icon: IconKind, color: Hsla) -> gpui::Stateful<gpui::Div> {
	div()
		.id(id)
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size(rem(CONTROL_HIT))
		.rounded_full()
		.cursor_pointer()
		.hover(|style| style.bg(white(0.12)))
		.child(Icon::new(icon).size(rem(CONTROL_ICON)).color(color))
}

fn center_button(id: ElementId, icon: IconKind, theme: &Theme) -> gpui::Stateful<gpui::Div> {
	div()
		.id(id)
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size(rem(CENTER_BUTTON))
		.rounded_full()
		.cursor_pointer()
		.bg(theme.overlay)
		.hover(|style| style.bg(black(0.85)))
		.child(
			Icon::new(icon)
				.size(rem(CENTER_BUTTON / 2.))
				.color(theme.text_on_brand),
		)
}

fn seek_bar(
	key: MediaKey,
	status: Option<Status>,
	scrub: Option<f32>,
	track: Hsla,
	fill: Hsla,
) -> AnyElement {
	let filled = share(status, scrub);
	div()
		.id(element_id("media-seek", key))
		.flex()
		.flex_1()
		.min_w(rem(40.))
		.items_center()
		.h(rem(BAR_HIT))
		.cursor_pointer()
		.on_click(|_, _, cx| cx.stop_propagation())
		.on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
			cx.stop_propagation();
			cx.global::<Playback>().scrub_at(key, event.position.x);
			window.refresh();
		})
		.child(
			div()
				.relative()
				.w_full()
				.h(rem(BAR_HEIGHT))
				.rounded_full()
				.bg(track)
				.child(
					div()
						.absolute()
						.left_0()
						.top_0()
						.bottom_0()
						.w(relative(filled))
						.rounded_full()
						.bg(fill),
				)
				.child(scrub_surface(key)),
		)
		.into_any_element()
}

fn scrub_surface(key: MediaKey) -> impl IntoElement {
	canvas(
		move |bounds: Bounds<Pixels>, _, cx| {
			let playback = cx.global::<Playback>();
			playback.set_bar(key, bounds);
			playback.scrub(key).is_some()
		},
		move |_, scrubbing: bool, window, _| {
			if !scrubbing {
				return;
			}
			window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
				if phase == DispatchPhase::Bubble && event.dragging() {
					cx.global::<Playback>().scrub_at(key, event.position.x);
					window.refresh();
				}
			});
			window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
				if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
					cx.global::<Playback>().release(key);
					window.refresh();
				}
			});
		},
	)
	.absolute()
	.size_full()
}

fn volume_control(key: MediaKey, color: Hsla, track: Hsla, theme: &Theme, cx: &App) -> AnyElement {
	let (volume, muted) = cx.global::<Playback>().volume();
	let silent = muted || volume <= 0.;
	let level = if silent { 0. } else { volume };
	let hover_group = group_name("media-volume", key);
	let label = cx.t(if silent { "media-unmute" } else { "media-mute" });
	div()
		.flex()
		.flex_none()
		.items_center()
		.group(hover_group.clone())
		.child(
			div()
				.id(element_id("media-volume-slider", key))
				.flex()
				.items_center()
				.w(rem(0.))
				.h(rem(BAR_HIT))
				.overflow_hidden()
				.cursor_pointer()
				.group_hover(hover_group, |style| style.w(rem(VOLUME_WIDTH)))
				.on_click(|_, _, cx| cx.stop_propagation())
				.on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, cx| {
					cx.stop_propagation();
					cx.global::<Playback>().volume_at(event.position.x);
					window.refresh();
				})
				.on_mouse_move(|event: &MouseMoveEvent, window, cx| {
					if event.dragging() {
						cx.global::<Playback>().volume_at(event.position.x);
						window.refresh();
					}
				})
				.child(
					div()
						.relative()
						.mx(rem(6.))
						.flex_1()
						.h(rem(BAR_HEIGHT))
						.rounded_full()
						.bg(track)
						.child(
							div()
								.absolute()
								.left_0()
								.top_0()
								.bottom_0()
								.w(relative(level))
								.rounded_full()
								.bg(theme.brand),
						)
						.child(
							canvas(
								|bounds: Bounds<Pixels>, _, cx| {
									cx.global::<Playback>().set_volume_bar(bounds)
								},
								|_, (), _, _| {},
							)
							.absolute()
							.size_full(),
						),
				),
		)
		.child(
			control(
				element_id("media-mute", key),
				if silent {
					IconKind::VolumeOff
				} else {
					IconKind::Volume
				},
				color,
			)
			.tooltip(crate::components::tooltip(label))
			.on_click(|_, window, cx| {
				cx.stop_propagation();
				cx.global::<Playback>().toggle_mute();
				window.refresh();
			}),
		)
		.into_any_element()
}

fn time_label(status: Option<Status>, scrub: Option<f32>, color: Hsla, theme: &Theme) -> AnyElement {
	let position = shown_position(status, scrub);
	let text = match status.and_then(|status| status.duration) {
		Some(duration) => format!("{} / {}", clock(position), clock(duration)),
		None => clock(position),
	};
	div()
		.flex_none()
		.text_size(theme.font_tiny)
		.font_weight(FontWeight::MEDIUM)
		.text_color(color)
		.child(text)
		.into_any_element()
}

fn tracker(key: MediaKey) -> AnyElement {
	canvas(
		|_, _, _| {},
		move |_, (), window, cx| cx.global::<Playback>().touch(key, window),
	)
	.absolute()
	.size_0()
	.into_any_element()
}

fn surface(key: MediaKey, radius: Pixels) -> AnyElement {
	canvas(
		|_, _, _| {},
		move |bounds, (), window, cx| cx.global::<Playback>().paint(key, bounds, radius, window),
	)
	.absolute()
	.size_full()
	.into_any_element()
}

fn poster(
	source: Option<&str>,
	placeholder: Option<&str>,
	width: f32,
	height: f32,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	match source {
		Some(url) => picture(url, placeholder, width, height, ObjectFit::Cover, theme, cx),
		None => div().size_full().bg(black(1.)).into_any_element(),
	}
}

pub fn video(source: VideoSource, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let key = source.key;
	let playback = cx.global::<Playback>();
	let status = playback.status(key);
	let scrub = playback.scrub(key);
	let showing_frame = playback.has_frame(key);
	let state = status.map(|status| status.state);
	let started = matches!(state, Some(State::Playing | State::Paused | State::Ended));
	let failed = state == Some(State::Failed);
	let max_size = hint_size(source.width, source.height);
	let radius = theme.pixels(theme.radius_md);
	let group = group_name("media-video", key);
	let controls_pinned = matches!(state, Some(State::Paused | State::Ended)) || scrub.is_some();
	let center = match state {
		None | Some(State::Ended) => Some(play_icon(status)),
		_ => None,
	};
	div()
		.id(element_id("media-video", key))
		.group(group.clone())
		.relative()
		.mt(theme.space_4)
		.w(rem(source.width))
		.h(rem(source.height))
		.max_w_full()
		.rounded(theme.radius_md)
		.overflow_hidden()
		.bg(black(1.))
		.cursor_pointer()
		.on_click(toggler(key, source.url.clone(), max_size))
		.when(!showing_frame || failed, |frame| {
			frame.child(poster(
				source.poster.as_deref(),
				source.placeholder,
				source.width,
				source.height,
				theme,
				cx,
			))
		})
		.child(surface(key, radius))
		.when_some(center.filter(|_| !failed), |frame, icon| {
			frame.child(
				div()
					.absolute()
					.inset_0()
					.flex()
					.items_center()
					.justify_center()
					.child(
						center_button(element_id("media-center", key), icon, theme).on_click(toggler(
							key,
							source.url.clone(),
							max_size,
						)),
					),
			)
		})
		.when(failed, |frame| frame.child(failure(env, cx)))
		.when_some(source.file, |frame, file| {
			frame.child(header(key, file, group.clone(), env))
		})
		.when(started, |frame| {
			frame.child(
				div()
					.absolute()
					.left_0()
					.right_0()
					.bottom_0()
					.h(rem(CONTROLS_HEIGHT))
					.flex()
					.items_center()
					.gap(theme.space_8)
					.px(theme.space_8)
					.bg(black(0.6))
					.when(!controls_pinned, |bar| {
						bar.invisible()
							.group_hover(group.clone(), |style| style.visible())
					})
					.child(
						control(element_id("media-toggle", key), play_icon(status), white(1.))
							.tooltip(crate::components::tooltip(play_label(status, cx)))
							.on_click(toggler(key, source.url.clone(), max_size)),
					)
					.child(time_label(status, scrub, white(1.), theme))
					.child(seek_bar(key, status, scrub, white(0.3), theme.brand))
					.child(volume_control(key, white(1.), white(0.3), theme, cx)),
			)
		})
		.into_any_element()
}

fn header(key: MediaKey, file: FileInfo, group: SharedString, env: &BodyEnv) -> AnyElement {
	let theme = env.theme;
	let dispatch = env.dispatch.clone();
	let download = file.download;
	div()
		.absolute()
		.left_0()
		.right_0()
		.top_0()
		.h(rem(HEADER_HEIGHT))
		.flex()
		.items_center()
		.gap(theme.space_8)
		.px(theme.space_12)
		.bg(black(0.6))
		.invisible()
		.group_hover(group, |style| style.visible())
		.child(
			div()
				.flex()
				.flex_col()
				.flex_1()
				.min_w_0()
				.child(
					div()
						.truncate()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(white(1.))
						.child(file.name),
				)
				.children(file.size.map(|size| {
					div()
						.text_size(theme.font_tiny)
						.text_color(white(0.7))
						.child(size)
				})),
		)
		.child(
			control(element_id("media-download", key), IconKind::Download, white(1.)).on_click(
				move |_, window, cx| {
					cx.stop_propagation();
					dispatch(RowAction::Download(download.clone()), window, cx);
				},
			),
		)
		.into_any_element()
}

fn failure(env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	div()
		.absolute()
		.inset_0()
		.flex()
		.flex_col()
		.items_center()
		.justify_center()
		.gap(theme.space_8)
		.bg(black(0.6))
		.child(Icon::new(IconKind::Warning).size(rem(28.)).color(white(0.9)))
		.child(
			div()
				.text_size(theme.font_sm)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(white(0.9))
				.child(cx.t("media-unsupported")),
		)
		.into_any_element()
}

pub fn gif_loop(source: LoopSource, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let key = source.key;
	let radius = theme.pixels(theme.radius_md);
	let showing = cx.global::<Playback>().has_frame(key);
	let url = source.url;
	div()
		.relative()
		.mt(theme.space_4)
		.w(rem(source.width))
		.h(rem(source.height))
		.max_w_full()
		.rounded(theme.radius_md)
		.overflow_hidden()
		.when(!showing, |frame| {
			frame.child(poster(
				source.poster.as_deref(),
				source.placeholder,
				source.width,
				source.height,
				theme,
				cx,
			))
		})
		.child(
			canvas(
				|_, _, _| {},
				move |bounds, (), window, cx| {
					cx.global::<Playback>()
						.paint_loop(key, &url, bounds, radius, window);
				},
			)
			.absolute()
			.size_full(),
		)
		.into_any_element()
}

pub fn audio(source: AudioSource, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let key = source.key;
	let playback = cx.global::<Playback>();
	let status = playback.status(key);
	let scrub = playback.scrub(key);
	let failed = status.is_some_and(|status| status.state == State::Failed);
	let dispatch = env.dispatch.clone();
	let download_dispatch = env.dispatch.clone();
	let download = source.file.download.clone();
	let open = source.file.download.clone();
	let icon_color = theme.interactive_normal;
	div()
		.id(element_id("media-audio", key))
		.relative()
		.mt(theme.space_4)
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.w(rem(AUDIO_WIDTH))
		.max_w_full()
		.p(theme.space_12)
		.rounded(theme.radius_md)
		.border_1()
		.border_color(theme.code_border)
		.bg(theme.background_secondary)
		.child(tracker(key))
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_12)
				.child(
					Icon::new(IconKind::FileAudio)
						.size(rem(AUDIO_ICON))
						.color(theme.interactive_normal),
				)
				.child(
					div()
						.flex()
						.flex_col()
						.flex_1()
						.min_w_0()
						.child(
							div()
								.id(element_id("media-audio-name", key))
								.truncate()
								.cursor_pointer()
								.text_size(theme.font_md)
								.text_color(theme.link)
								.hover(|style| style.underline())
								.on_click(move |_, window, cx| {
									dispatch(RowAction::Download(open.clone()), window, cx)
								})
								.child(source.file.name),
						)
						.children(source.file.size.map(|size| {
							div()
								.text_size(theme.font_tiny)
								.text_color(theme.text_muted)
								.child(size)
						})),
				)
				.child(
					control(
						element_id("media-audio-download", key),
						IconKind::Download,
						icon_color,
					)
					.tooltip(crate::components::tooltip(cx.t("attachment-download")))
					.on_click(move |_, window, cx| {
						download_dispatch(RowAction::Download(download.clone()), window, cx)
					}),
				),
		)
		.child(if failed {
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.text_size(theme.font_sm)
				.text_color(theme.text_muted)
				.child(
					Icon::new(IconKind::Warning)
						.size(rem(16.))
						.color(theme.text_muted),
				)
				.child(cx.t("media-unsupported"))
				.into_any_element()
		} else {
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(
					control(
						element_id("media-audio-toggle", key),
						play_icon(status),
						icon_color,
					)
					.tooltip(crate::components::tooltip(play_label(status, cx)))
					.on_click(toggler(key, source.url, (0, 0))),
				)
				.child(time_label(status, scrub, theme.text_muted, theme))
				.child(seek_bar(key, status, scrub, theme.interactive_muted, theme.brand))
				.child(volume_control(
					key,
					icon_color,
					theme.interactive_muted,
					theme,
					cx,
				))
				.into_any_element()
		})
		.into_any_element()
}

fn waveform_levels(encoded: Option<&str>) -> Vec<f32> {
	let samples = encoded
		.and_then(|encoded| STANDARD.decode(encoded).ok())
		.filter(|samples| !samples.is_empty())
		.unwrap_or_else(|| vec![u8::MAX / 4; VOICE_BARS]);
	(0..VOICE_BARS)
		.map(|bar| {
			let start = bar * samples.len() / VOICE_BARS;
			let end = ((bar + 1) * samples.len() / VOICE_BARS)
				.max(start + 1)
				.min(samples.len());
			let peak = samples[start..end].iter().copied().max().unwrap_or_default();
			f32::from(peak) / WAVEFORM_PEAK
		})
		.collect()
}

pub fn voice_message(source: VoiceSource, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let key = source.key;
	let playback = cx.global::<Playback>();
	let status = playback.status(key);
	let scrub = playback.scrub(key);
	let played = share(status, scrub);
	let playing = status.is_some_and(|status| matches!(status.state, State::Playing | State::Loading));
	let total = status
		.and_then(|status| status.duration)
		.or_else(|| source.duration.map(Duration::from_secs_f64))
		.unwrap_or_default();
	let label = if status.is_some_and(|status| status.state != State::Ended) {
		clock(shown_position(status, scrub))
	} else {
		clock(total)
	};
	let levels = waveform_levels(source.waveform);
	let count = levels.len() as f32;
	let url = source.url.clone();
	div()
		.id(element_id("media-voice", key))
		.relative()
		.mt(theme.space_4)
		.flex()
		.items_center()
		.gap(theme.space_8)
		.w(rem(VOICE_WIDTH))
		.max_w_full()
		.h(rem(VOICE_HEIGHT))
		.px(theme.space_8)
		.rounded_full()
		.bg(theme.background_secondary)
		.child(tracker(key))
		.child(
			div()
				.id(element_id("media-voice-toggle", key))
				.flex()
				.flex_none()
				.items_center()
				.justify_center()
				.size(rem(VOICE_BUTTON))
				.rounded_full()
				.cursor_pointer()
				.bg(theme.brand)
				.hover(|style| style.bg(theme.brand_hover))
				.tooltip(crate::components::tooltip(play_label(status, cx)))
				.on_click(toggler(key, source.url, (0, 0)))
				.child(
					Icon::new(if playing { IconKind::Pause } else { IconKind::Play })
						.size(rem(16.))
						.color(theme.text_on_brand),
				),
		)
		.child(
			div()
				.id(element_id("media-voice-wave", key))
				.relative()
				.flex()
				.flex_1()
				.min_w_0()
				.items_center()
				.justify_between()
				.h(rem(VOICE_BAR_MAX))
				.cursor_pointer()
				.on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
					cx.stop_propagation();
					let playback = cx.global::<Playback>();
					playback.scrub_at(key, event.position.x);
					if playback.status(key).is_none() {
						playback.toggle(key, &url, (0, 0));
					}
					window.refresh();
				})
				.children(levels.into_iter().enumerate().map(|(index, level)| {
					let lit = (index as f32 + 0.5) / count <= played && status.is_some();
					div()
						.flex_none()
						.w(rem(VOICE_BAR_WIDTH))
						.h(rem(VOICE_BAR_MIN + level * (VOICE_BAR_MAX - VOICE_BAR_MIN)))
						.rounded_full()
						.bg(if lit {
							theme.header_primary
						} else {
							theme.interactive_muted
						})
				}))
				.child(scrub_surface(key)),
		)
		.child(
			div()
				.flex_none()
				.min_w(rem(36.))
				.text_size(theme.font_sm)
				.font_weight(FontWeight::MEDIUM)
				.text_color(theme.header_secondary)
				.child(label),
		)
		.into_any_element()
}
