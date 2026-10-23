use std::sync::Arc;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
	ObjectFit, ParentElement as _, Render, RenderImage, SharedString, StatefulInteractiveElement as _,
	Styled as _, StyledImage as _, Task, Window, div, img, prelude::FluentBuilder as _, relative,
};
use gpui_component::Disableable as _;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::switch::Switch;
use session::{Command, ScreenSource, ScreenTarget, ScreenTargetKind};
use state::VideoKind;

use super::quality::Preset;
use super::video_sink::{PreviewSource, Textures, thumbnail};
use crate::components::{IconButton, tooltip};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const AUDIO_SUPPORTED: bool = cfg!(windows);
const MAX_WINDOWS: usize = 24;
const DIALOG_MARGIN_TOP: f32 = 64.;
const GRID_HEIGHT_RATIO: f32 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
	Screens,
	Windows,
}

struct Item {
	target: ScreenTarget,
	thumb: Option<Arc<RenderImage>>,
}

pub struct SharePicker {
	data: Entity<Data>,
	tab: Tab,
	items: Vec<Item>,
	selected: Option<usize>,
	preset: Preset,
	share_audio: bool,
	loading: bool,
	_load: Task<()>,
}

pub fn open(data: &Entity<Data>, preset: Preset, window: &mut Window, cx: &mut gpui::App) {
	if window.has_active_dialog(cx) {
		return;
	}
	let picker = cx.new(|cx| SharePicker::new(data.clone(), preset, cx));
	let width = cx.global::<Theme>().voice_picker_width;
	window.open_dialog(cx, move |dialog, window, _| {
		dialog
			.w(width.to_pixels(window.rem_size()))
			.margin_top(rem(DIALOG_MARGIN_TOP).to_pixels(window.rem_size()))
			.close_button(false)
			.on_ok(|_, _, _| false)
			.child(picker.clone())
	});
}

impl SharePicker {
	fn new(data: Entity<Data>, preset: Preset, cx: &mut Context<Self>) -> Self {
		cx.on_release(|picker, cx| {
			for item in picker.items.drain(..) {
				if let Some(thumb) = item.thumb {
					cx.drop_image(thumb, None);
				}
			}
		})
		.detach();
		let load = cx.spawn(async move |this, cx| {
			let listed = cx
				.background_executor()
				.spawn(async { ScreenSource::list() })
				.await;
			let mut targets = listed.unwrap_or_default();
			let mut windows = 0;
			targets.retain(|target| {
				if target.kind == ScreenTargetKind::Window {
					windows += 1;
					windows <= MAX_WINDOWS
				} else {
					true
				}
			});
			let shown = targets.clone();
			let ready = this.update(cx, |picker, cx| {
				picker.items = shown
					.into_iter()
					.map(|target| Item { target, thumb: None })
					.collect();
				picker.selected = picker
					.items
					.iter()
					.position(|item| item.target.kind == ScreenTargetKind::Monitor && item.target.primary)
					.or_else(|| {
						picker
							.items
							.iter()
							.position(|item| item.target.kind == ScreenTargetKind::Monitor)
					});
				picker.loading = false;
				cx.notify();
			});
			if ready.is_err() {
				return;
			}
			for (index, target) in targets.into_iter().enumerate() {
				let thumb = cx
					.background_executor()
					.spawn(async move { thumbnail(&target) })
					.await;
				let applied = this.update(cx, |picker, cx| {
					if let Some(item) = picker.items.get_mut(index) {
						item.thumb = thumb;
					}
					cx.notify();
				});
				if applied.is_err() {
					return;
				}
			}
		});
		Self {
			data,
			tab: Tab::Screens,
			items: Vec::new(),
			selected: None,
			preset,
			share_audio: false,
			loading: true,
			_load: load,
		}
	}

	fn visible(&self) -> impl Iterator<Item = (usize, &Item)> {
		let wanted = match self.tab {
			Tab::Screens => ScreenTargetKind::Monitor,
			Tab::Windows => ScreenTargetKind::Window,
		};
		self.items
			.iter()
			.enumerate()
			.filter(move |(_, item)| item.target.kind == wanted)
	}

	fn select_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
		self.tab = tab;
		let first = self.visible().next().map(|(index, _)| index);
		self.selected = first;
		cx.notify();
	}

	fn go_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(item) = self.selected.and_then(|index| self.items.get(index)) else {
			return;
		};
		let target = item.target.clone();
		let quality = self.preset.quality(Some((target.width, target.height)));
		let (connected, me) = {
			let data = self.data.read(cx);
			(data.state().voice().is_some(), data.me())
		};
		self.data.read(cx).send(Command::StartScreenShare {
			target: target.clone(),
			quality,
			with_audio: self.share_audio,
		});
		if connected && let (Some(textures), Some(me)) = (cx.try_global::<Textures>().cloned(), me) {
			textures
				.0
				.start_preview((me, VideoKind::Screen), PreviewSource::Screen(target));
		}
		window.close_dialog(cx);
	}

	fn card(&self, index: usize, item: &Item, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let selected = self.selected == Some(index);
		let border = if selected {
			theme.brand
		} else {
			theme.background_tertiary
		};
		let preview = match &item.thumb {
			Some(image) => div().size_full().child(
				img(image.clone())
					.size_full()
					.object_fit(ObjectFit::Contain)
					.rounded(theme.radius_sm),
			),
			None => div().size_full().flex().items_center().justify_center().child(
				Icon::new(match item.target.kind {
					ScreenTargetKind::Monitor => IconKind::Devices,
					ScreenTargetKind::Window => IconKind::AppWindow,
				})
				.size(theme.voice_icon_lg)
				.color(theme.interactive_muted),
			),
		};
		div()
			.w(relative(1. / 3.))
			.p(theme.space_4)
			.child(
				div()
					.id(("voice-picker-target", index))
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.p(theme.space_8)
					.rounded(theme.radius_md)
					.border_2()
					.border_color(border)
					.bg(theme.background_tertiary)
					.cursor_pointer()
					.on_click(cx.listener(move |this, _, _, cx| {
						this.selected = Some(index);
						cx.notify();
					}))
					.child(
						div()
							.h(theme.voice_thumb_height)
							.rounded(theme.radius_sm)
							.bg(theme.background_floating)
							.overflow_hidden()
							.child(preview),
					)
					.child(
						div()
							.truncate()
							.text_size(theme.font_xs)
							.text_color(theme.text_normal)
							.child(SharedString::from(item.target.name.clone())),
					),
			)
			.into_any_element()
	}

	fn tab_button(&self, tab: Tab, label: SharedString, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let active = self.tab == tab;
		div()
			.id(match tab {
				Tab::Screens => "voice-picker-screens",
				Tab::Windows => "voice-picker-windows",
			})
			.px(theme.space_16)
			.py(theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_sm)
			.font_weight(FontWeight::SEMIBOLD)
			.when(active, |button| {
				button
					.bg(theme.modifier_selected)
					.text_color(theme.interactive_active)
			})
			.when(!active, |button| {
				button
					.text_color(theme.interactive_normal)
					.hover(|style| style.bg(theme.modifier_hover))
			})
			.on_click(cx.listener(move |this, _, _, cx| this.select_tab(tab, cx)))
			.child(label)
			.into_any_element()
	}

	fn preset_chip(&self, preset: Preset, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let active = self.preset == preset;
		div()
			.id(("voice-picker-quality", usize::from(preset.index())))
			.px(theme.space_12)
			.py(theme.space_4)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_xs)
			.font_weight(FontWeight::MEDIUM)
			.when(active, |chip| {
				chip.bg(theme.brand).text_color(theme.text_on_brand)
			})
			.when(!active, |chip| {
				chip.bg(theme.modifier_selected)
					.text_color(theme.interactive_normal)
					.hover(|style| style.bg(theme.modifier_active))
			})
			.on_click(cx.listener(move |this, _, _, cx| {
				this.preset = preset;
				cx.notify();
			}))
			.child(cx.t(preset.key()))
			.into_any_element()
	}
}

impl Render for SharePicker {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let cards: Vec<_> = self
			.visible()
			.map(|(index, item)| self.card(index, item, &theme, cx))
			.collect();
		let chips: Vec<AnyElement> = Preset::ALL
			.iter()
			.map(|preset| self.preset_chip(*preset, &theme, cx))
			.collect();
		let tab_screens = self.tab_button(Tab::Screens, cx.t("voice-picker-screens"), &theme, cx);
		let tab_windows = self.tab_button(Tab::Windows, cx.t("voice-picker-windows"), &theme, cx);
		let empty = cards.is_empty();
		let can_share = self.selected.is_some();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.child(
						div()
							.text_size(theme.font_lg)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(cx.t("voice-picker-title")),
					)
					.child(
						IconButton::new("voice-picker-close", IconKind::Close)
							.filled_hover()
							.tooltip(cx.t("lightbox-close"))
							.on_click(|_, window, cx| window.close_dialog(cx)),
					),
			)
			.child(
				div()
					.flex()
					.gap(theme.space_8)
					.child(tab_screens)
					.child(tab_windows),
			)
			.child(
				div()
					.id("voice-picker-grid")
					.flex()
					.flex_wrap()
					.content_start()
					.h(theme.voice_picker_width * GRID_HEIGHT_RATIO)
					.overflow_y_scroll()
					.children(cards)
					.when(empty, |grid| {
						grid.items_center().justify_center().child(
							div()
								.w_full()
								.text_center()
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(cx.t(if self.loading {
									"voice-picker-loading"
								} else {
									"voice-picker-empty"
								})),
						)
					}),
			)
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.gap(theme.space_16)
					.child(
						div()
							.flex()
							.flex_col()
							.gap(theme.space_8)
							.child(
								div()
									.text_size(theme.font_tiny)
									.font_weight(FontWeight::BOLD)
									.text_color(theme.header_secondary)
									.child(cx.t("voice-picker-quality").to_uppercase()),
							)
							.child(div().flex().gap(theme.space_8).children(chips)),
					)
					.child(
						div()
							.id("voice-picker-audio")
							.tooltip(tooltip(cx.t(if AUDIO_SUPPORTED {
								"voice-picker-audio-note"
							} else {
								"voice-picker-audio-unavailable"
							})))
							.child(
								Switch::new("voice-picker-share-audio")
									.checked(self.share_audio)
									.disabled(!AUDIO_SUPPORTED)
									.label(cx.t("voice-picker-share-audio"))
									.on_change(cx.listener(|this, checked: &bool, _, cx| {
										this.share_audio = *checked;
										cx.notify();
									})),
							),
					),
			)
			.child(
				div()
					.flex()
					.items_center()
					.justify_end()
					.gap(theme.space_8)
					.child(
						Button::new("voice-picker-cancel")
							.ghost()
							.label(cx.t("action-cancel"))
							.on_click(|_, window, cx| window.close_dialog(cx)),
					)
					.child(
						Button::new("voice-picker-go-live")
							.primary()
							.label(cx.t("voice-picker-go-live"))
							.disabled(!can_share)
							.on_click(cx.listener(|this, _, window, cx| this.go_live(window, cx))),
					),
			)
	}
}
