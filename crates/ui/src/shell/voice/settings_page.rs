use std::rc::Rc;
use std::time::Duration;

use gpui::{
	AppContext as _, Context, Entity, FontWeight, IntoElement, ObjectFit, ParentElement as _, Render,
	SharedString, Styled as _, StyledImage as _, Subscription, Task, Window, div, img,
	prelude::FluentBuilder as _, relative,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::radio::{Radio, RadioGroup};
use gpui_component::searchable_list::SearchableListItem;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{Disableable as _, IndexPath};
use session::{CameraSource, Command};
use state::VideoKind;
use store::VoiceSettings;
use voice::AudioDevices;

use super::quality::Preset;
use super::video_sink::{PREVIEW_USER, PreviewSource, Textures};
use crate::data::Data;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::SettingsBackend;
use crate::theme::Theme;

const TEST_SECONDS: u32 = 3;
const SLIDER_DEBOUNCE: Duration = Duration::from_millis(100);
const VOLUME_MAX_PERCENT: f32 = 200.;
const SENSITIVITY_MIN_DB: f32 = -100.;
const SENSITIVITY_MAX_DB: f32 = 0.;
const METER_FLOOR_DB: f32 = -100.;
const METER_SILENCE: f32 = 1e-5;

#[derive(Clone)]
struct Choice {
	label: SharedString,
	value: String,
}

impl SearchableListItem for Choice {
	type Value = String;

	fn title(&self) -> SharedString {
		self.label.clone()
	}

	fn value(&self) -> &String {
		&self.value
	}
}

type ChoiceSelect = Entity<SelectState<Vec<Choice>>>;

pub struct VoiceVideoPage {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	voice: VoiceSettings,
	inputs: ChoiceSelect,
	outputs: ChoiceSelect,
	cameras: ChoiceSelect,
	presets: ChoiceSelect,
	input_volume: Entity<SliderState>,
	output_volume: Entity<SliderState>,
	sensitivity: Entity<SliderState>,
	camera_count: usize,
	_input_apply: Task<()>,
	_output_apply: Task<()>,
	_sensitivity_apply: Task<()>,
	_load: Task<()>,
	_frames: Task<()>,
	_subscriptions: Vec<Subscription>,
}

fn default_choice(cx: &Context<VoiceVideoPage>) -> Choice {
	Choice {
		label: cx.t("voice-device-default"),
		value: String::new(),
	}
}

fn choice_select(
	items: Vec<Choice>,
	selected: &str,
	window: &mut Window,
	cx: &mut Context<VoiceVideoPage>,
) -> ChoiceSelect {
	let index = items.iter().position(|item| item.value == selected).unwrap_or(0);
	cx.new(|cx| SelectState::new(items, Some(IndexPath::default().row(index)), window, cx))
}

fn slider(min: f32, max: f32, start: f32, cx: &mut Context<VoiceVideoPage>) -> Entity<SliderState> {
	cx.new(|_| {
		SliderState::new()
			.min(min)
			.max(max)
			.step(1.)
			.default_value(start.clamp(min, max))
	})
}

fn defer(
	cx: &mut Context<VoiceVideoPage>,
	apply: impl FnOnce(&mut VoiceVideoPage, &mut Context<VoiceVideoPage>) + 'static,
) -> Task<()> {
	cx.spawn(async move |this, cx| {
		cx.background_executor().timer(SLIDER_DEBOUNCE).await;
		this.update(cx, |page, cx| apply(page, cx)).ok();
	})
}

fn to_option(value: &str) -> Option<String> {
	Some(value.to_owned()).filter(|value| !value.is_empty())
}

fn level_fraction(level: f32) -> f32 {
	let db = 20. * level.max(METER_SILENCE).log10();
	((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0., 1.)
}

fn db_fraction(db: f32) -> f32 {
	((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0., 1.)
}

impl VoiceVideoPage {
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let voice = (backend.read)().voice;
		let inputs = choice_select(
			vec![default_choice(cx)],
			voice.input_device.as_deref().unwrap_or_default(),
			window,
			cx,
		);
		let outputs = choice_select(
			vec![default_choice(cx)],
			voice.output_device.as_deref().unwrap_or_default(),
			window,
			cx,
		);
		let cameras = choice_select(
			vec![default_choice(cx)],
			voice.camera_device.as_deref().unwrap_or_default(),
			window,
			cx,
		);
		let preset_items: Vec<Choice> = Preset::ALL
			.iter()
			.map(|preset| Choice {
				label: cx.t(preset.key()),
				value: preset.index().to_string(),
			})
			.collect();
		let presets = choice_select(preset_items, &voice.video_preset.to_string(), window, cx);
		let input_volume = slider(0., VOLUME_MAX_PERCENT, f32::from(voice.input_volume), cx);
		let output_volume = slider(0., VOLUME_MAX_PERCENT, f32::from(voice.output_volume), cx);
		let sensitivity = slider(
			SENSITIVITY_MIN_DB,
			SENSITIVITY_MAX_DB,
			f32::from(voice.sensitivity_db),
			cx,
		);
		let mut subscriptions = vec![
			cx.subscribe_in(
				&inputs,
				window,
				|this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
					let SelectEvent::Confirm(value) = event;
					if let Some(value) = value {
						this.voice.input_device = to_option(value);
						this.apply_devices(cx);
					}
				},
			),
			cx.subscribe_in(
				&outputs,
				window,
				|this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
					let SelectEvent::Confirm(value) = event;
					if let Some(value) = value {
						this.voice.output_device = to_option(value);
						this.apply_devices(cx);
					}
				},
			),
			cx.subscribe_in(
				&cameras,
				window,
				|this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
					let SelectEvent::Confirm(value) = event;
					if let Some(value) = value {
						let device = to_option(value);
						this.voice.camera_device = device.clone();
						this.write(move |voice| voice.camera_device = device.clone());
						this.restart_camera_test(cx);
						cx.notify();
					}
				},
			),
			cx.subscribe_in(
				&presets,
				window,
				|this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
					let SelectEvent::Confirm(value) = event;
					if let Some(preset) = value.as_ref().and_then(|value| value.parse::<u8>().ok()) {
						this.voice.video_preset = preset;
						this.write(move |voice| voice.video_preset = preset);
						this.data.read(cx).send(Command::SetDefaultVideoQuality(
							Preset::from_index(preset).quality(None),
						));
						cx.notify();
					}
				},
			),
			cx.subscribe(&input_volume, |this, _, event: &SliderEvent, cx| {
				if let SliderEvent::Change(value) = event {
					this.voice.input_volume = value.end().round() as u8;
					this._input_apply = defer(cx, |page, cx| {
						let volume = f32::from(page.voice.input_volume) / 100.;
						page.data.read(cx).send(Command::SetInputVolume(volume));
					});
					cx.notify();
				}
			}),
			cx.subscribe(&output_volume, |this, _, event: &SliderEvent, cx| {
				if let SliderEvent::Change(value) = event {
					this.voice.output_volume = value.end().round() as u8;
					this._output_apply = defer(cx, |page, cx| {
						let volume = f32::from(page.voice.output_volume) / 100.;
						page.data.read(cx).send(Command::SetOutputVolume(volume));
					});
					cx.notify();
				}
			}),
			cx.subscribe(&sensitivity, |this, _, event: &SliderEvent, cx| {
				if let SliderEvent::Change(value) = event {
					this.voice.sensitivity_db = value.end().round() as i8;
					this._sensitivity_apply = defer(cx, |page, cx| page.send_vad_threshold(cx));
					cx.notify();
				}
			}),
		];
		subscriptions.push(cx.observe(&data, |_, _, cx| cx.notify()));
		let load = cx.spawn_in(window, async move |this, cx| {
			let devices = cx
				.background_executor()
				.spawn(async { (voice::devices(), CameraSource::list().unwrap_or_default()) })
				.await;
			this.update_in(cx, |page, window, cx| {
				page.set_devices(devices.0, devices.1, window, cx)
			})
			.ok();
		});
		let frames = match cx.try_global::<Textures>().cloned() {
			Some(textures) => cx.spawn(async move |this, cx| {
				loop {
					textures.0.changed().await;
					let retired = textures.0.take_retired();
					let alive = this
						.update(cx, |_, cx| {
							for image in retired {
								cx.drop_image(image, None);
							}
							cx.notify();
						})
						.is_ok();
					if !alive {
						break;
					}
				}
			}),
			None => Task::ready(()),
		};
		cx.on_release(|_, cx| {
			if let Some(textures) = cx.try_global::<Textures>().cloned() {
				textures.0.stop_preview((PREVIEW_USER, VideoKind::Camera));
			}
		})
		.detach();
		Self {
			data,
			backend,
			voice,
			inputs,
			outputs,
			cameras,
			presets,
			input_volume,
			output_volume,
			sensitivity,
			camera_count: 0,
			_input_apply: Task::ready(()),
			_output_apply: Task::ready(()),
			_sensitivity_apply: Task::ready(()),
			_load: load,
			_frames: frames,
			_subscriptions: subscriptions,
		}
	}

	fn write(&self, apply: impl Fn(&mut VoiceSettings) + 'static) {
		(self.backend.write)(&move |settings| apply(&mut settings.voice));
	}

	fn set_devices(
		&mut self,
		devices: AudioDevices,
		cameras: Vec<session::CameraInfo>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let list = |cx: &Context<Self>, devices: Vec<voice::DeviceInfo>| {
			let mut items = vec![default_choice(cx)];
			items.extend(devices.into_iter().map(|device| Choice {
				label: device.name.into(),
				value: device.id,
			}));
			items
		};
		let input_items = list(cx, devices.inputs);
		let output_items = list(cx, devices.outputs);
		let mut camera_items = vec![default_choice(cx)];
		camera_items.extend(cameras.into_iter().map(|camera| Choice {
			label: camera.name.into(),
			value: camera.id,
		}));
		self.camera_count = camera_items.len() - 1;
		let input = self.voice.input_device.clone().unwrap_or_default();
		let output = self.voice.output_device.clone().unwrap_or_default();
		let camera = self.voice.camera_device.clone().unwrap_or_default();
		for (state, items, selected) in [
			(&self.inputs, input_items, input),
			(&self.outputs, output_items, output),
			(&self.cameras, camera_items, camera),
		] {
			state.update(cx, |state, cx| {
				state.set_items(items, window, cx);
				state.set_selected_value(&selected, window, cx);
			});
		}
		cx.notify();
	}

	fn apply_devices(&mut self, cx: &mut Context<Self>) {
		self.data.read(cx).send(Command::SetVoiceDevices {
			input: self.voice.input_device.clone(),
			output: self.voice.output_device.clone(),
		});
		cx.notify();
	}

	fn set_input_mode(&mut self, push_to_talk: bool, cx: &mut Context<Self>) {
		self.voice.push_to_talk = push_to_talk;
		self.write(move |voice| voice.push_to_talk = push_to_talk);
		self.data
			.update(cx, |data, cx| data.set_input_mode(push_to_talk, cx));
		cx.notify();
	}

	fn set_noise_suppression(&mut self, enabled: bool, cx: &mut Context<Self>) {
		self.voice.noise_suppression = enabled;
		self.data.read(cx).send(Command::SetNoiseSuppression(enabled));
		cx.notify();
	}

	fn send_vad_threshold(&self, cx: &Context<Self>) {
		self.data.read(cx).send(Command::SetVadThreshold {
			auto: self.voice.auto_sensitivity,
			threshold_db: f32::from(self.voice.sensitivity_db),
		});
	}

	fn set_auto_sensitivity(&mut self, enabled: bool, cx: &mut Context<Self>) {
		self.voice.auto_sensitivity = enabled;
		self.send_vad_threshold(cx);
		cx.notify();
	}

	fn toggle_test(&self, testing: bool, cx: &Context<Self>) {
		let command = if testing {
			Command::StopMicTest
		} else {
			Command::StartMicTest {
				seconds: TEST_SECONDS,
			}
		};
		self.data.read(cx).send(command);
	}

	fn camera_testing(&self, cx: &gpui::App) -> bool {
		cx.try_global::<Textures>()
			.is_some_and(|textures| textures.0.is_previewing((PREVIEW_USER, VideoKind::Camera)))
	}

	fn toggle_camera_test(&mut self, cx: &mut Context<Self>) {
		let Some(textures) = cx.try_global::<Textures>().cloned() else {
			return;
		};
		let key = (PREVIEW_USER, VideoKind::Camera);
		if textures.0.is_previewing(key) {
			textures.0.stop_preview(key);
		} else {
			textures
				.0
				.start_preview(key, PreviewSource::Camera(self.voice.camera_device.clone()));
		}
		cx.notify();
	}

	fn restart_camera_test(&mut self, cx: &mut Context<Self>) {
		let Some(textures) = cx.try_global::<Textures>().cloned() else {
			return;
		};
		let key = (PREVIEW_USER, VideoKind::Camera);
		if textures.0.is_previewing(key) {
			textures.0.stop_preview(key);
			textures
				.0
				.start_preview(key, PreviewSource::Camera(self.voice.camera_device.clone()));
		}
	}

	fn label(&self, key: &str, theme: &Theme, cx: &Context<Self>) -> impl IntoElement {
		div()
			.text_size(theme.font_tiny)
			.font_weight(FontWeight::BOLD)
			.text_color(theme.header_secondary)
			.child(cx.t(key).to_uppercase())
	}

	fn field(&self, key: &str, body: impl IntoElement, theme: &Theme, cx: &Context<Self>) -> gpui::Div {
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(self.label(key, theme, cx))
			.child(body)
	}

	fn volume_row(&self, state: &Entity<SliderState>, percent: u8, theme: &Theme) -> impl IntoElement {
		div()
			.flex()
			.items_center()
			.gap(theme.space_16)
			.child(div().flex_1().child(Slider::new(state)))
			.child(
				div()
					.w(theme.space_32 * 2.)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(format!("{percent}%")),
			)
	}

	fn meter(&self, fraction: f32, threshold: Option<f32>, theme: &Theme) -> impl IntoElement {
		div()
			.relative()
			.flex_1()
			.h(theme.voice_meter_height)
			.rounded_full()
			.overflow_hidden()
			.bg(theme.background_tertiary)
			.child(
				div()
					.h_full()
					.w(relative(fraction))
					.rounded_full()
					.bg(theme.success),
			)
			.when_some(threshold, |meter, threshold| {
				meter.child(
					div()
						.absolute()
						.top_0()
						.bottom_0()
						.left(relative(threshold))
						.w(theme.space_2)
						.bg(theme.interactive_active),
				)
			})
	}

	fn camera_preview(&self, theme: &Theme, cx: &Context<Self>) -> impl IntoElement {
		let image = cx
			.try_global::<Textures>()
			.and_then(|textures| textures.0.image((PREVIEW_USER, VideoKind::Camera)));
		let body = match image {
			Some(image) => div().size_full().child(
				img(image)
					.size_full()
					.object_fit(ObjectFit::Cover)
					.rounded(theme.radius_md),
			),
			None => div()
				.size_full()
				.flex()
				.flex_col()
				.items_center()
				.justify_center()
				.gap(theme.space_8)
				.text_size(theme.font_sm)
				.text_color(theme.text_muted)
				.child(
					Icon::new(IconKind::VideoOff)
						.size(theme.voice_icon_lg)
						.color(theme.interactive_muted),
				)
				.child(cx.t(if self.camera_count == 0 {
					"voice-camera-none"
				} else {
					"voice-camera-preview-off"
				})),
		};
		div()
			.w(theme.voice_field_width)
			.h(theme.voice_field_width * (9. / 16.))
			.rounded(theme.radius_md)
			.overflow_hidden()
			.bg(theme.background_tertiary)
			.child(body)
	}
}

impl Render for VoiceVideoPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (connected, level, camera_in_call, test) = {
			let data = self.data.read(cx);
			let state = data.state();
			let test = state.mic_test();
			match state.voice() {
				Some(voice) => (true, voice.levels.input, voice.camera_on, test),
				None => (false, 0., false, test),
			}
		};
		let testing = test.is_some();
		let meter_fraction = level_fraction(test.map_or(level, |test| test.level));
		let manual_sensitivity = !self.voice.push_to_talk && !self.voice.auto_sensitivity;
		let threshold = manual_sensitivity.then(|| db_fraction(f32::from(self.voice.sensitivity_db)));
		let camera_testing = self.camera_testing(cx);
		let input_percent = self.voice.input_volume;
		let output_percent = self.voice.output_volume;
		let mode_index = usize::from(self.voice.push_to_talk);
		let device_row = div()
			.flex()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap(theme.space_16)
					.child(self.field(
						"voice-input-device",
						Select::new(&self.inputs).w_full(),
						&theme,
						cx,
					))
					.child(self.field(
						"voice-input-volume",
						self.volume_row(&self.input_volume, input_percent, &theme),
						&theme,
						cx,
					)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap(theme.space_16)
					.child(self.field(
						"voice-output-device",
						Select::new(&self.outputs).w_full(),
						&theme,
						cx,
					))
					.child(self.field(
						"voice-output-volume",
						self.volume_row(&self.output_volume, output_percent, &theme),
						&theme,
						cx,
					)),
			);
		let test_body = div()
			.flex()
			.items_center()
			.gap(theme.space_16)
			.when(!connected, |row| {
				row.child(
					Button::new("voice-mic-test")
						.primary()
						.when(testing, |button| button.outline())
						.when(!testing, |button| button.primary())
						.label(cx.t(if testing {
							"voice-mic-stop"
						} else {
							"voice-mic-check"
						}))
						.on_click(cx.listener(move |this, _, _, cx| this.toggle_test(testing, cx))),
				)
			})
			.child(self.meter(meter_fraction, threshold, &theme))
			.into_any_element();
		let test_hint = test.map(|test| {
			cx.t_args(
				"voice-mic-test-running",
				&[("seconds", ArgValue::from(test.remaining_secs as usize))],
			)
		});
		let mode_body =
			div()
				.flex()
				.flex_col()
				.gap(theme.space_12)
				.child(
					RadioGroup::vertical("voice-input-mode")
						.selected_index(Some(mode_index))
						.child(Radio::new("voice-mode-vad").label(cx.t("voice-mode-activity")))
						.child(Radio::new("voice-mode-ptt").label(cx.t("voice-mode-ptt")))
						.on_change(
							cx.listener(|this, index: &usize, _, cx| this.set_input_mode(*index == 1, cx)),
						),
				)
				.when(!self.voice.push_to_talk, |body| {
					body.child(
						Switch::new("voice-auto-sensitivity")
							.checked(self.voice.auto_sensitivity)
							.label(cx.t("voice-auto-sensitivity"))
							.on_change(cx.listener(|this, checked: &bool, _, cx| {
								this.set_auto_sensitivity(*checked, cx)
							})),
					)
				})
				.when(manual_sensitivity, |body| {
					body.child(
						self.field(
							"voice-sensitivity",
							div()
								.flex()
								.items_center()
								.gap(theme.space_16)
								.child(div().flex_1().child(Slider::new(&self.sensitivity)))
								.child(
									div()
										.w(theme.space_32 * 2.)
										.text_size(theme.font_sm)
										.text_color(theme.text_muted)
										.child(cx.t_args(
											"voice-decibels",
											&[("db", ArgValue::from(i64::from(self.voice.sensitivity_db)))],
										)),
								),
							&theme,
							cx,
						),
					)
				})
				.when(self.voice.push_to_talk, |body| {
					body.child(
						div()
							.text_size(theme.font_sm)
							.text_color(theme.text_muted)
							.child(cx.t("voice-ptt-hint")),
					)
				});
		let camera_test_disabled = self.camera_count == 0 || camera_in_call;
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(device_row)
			.child(
				self.field("voice-mic-test", test_body, &theme, cx)
					.children(test_hint.map(|hint| {
						div()
							.text_size(theme.font_xs)
							.text_color(theme.text_muted)
							.child(hint)
					})),
			)
			.child(self.field("voice-input-mode", mode_body, &theme, cx))
			.child(
				self.field(
					"voice-noise-suppression",
					Switch::new("voice-noise-suppression")
						.checked(self.voice.noise_suppression)
						.label(cx.t("voice-noise-suppression-label"))
						.on_change(cx.listener(|this, checked: &bool, _, cx| {
							this.set_noise_suppression(*checked, cx)
						})),
					&theme,
					cx,
				),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_12)
					.child(
						self.field(
							"voice-camera",
							div()
								.w(theme.voice_field_width)
								.child(Select::new(&self.cameras).w_full()),
							&theme,
							cx,
						),
					)
					.child(self.camera_preview(&theme, cx))
					.child(
						div().flex().child(
							Button::new("voice-camera-test")
								.disabled(camera_test_disabled)
								.when(camera_testing, |button| button.primary())
								.label(cx.t(if camera_testing {
									"voice-camera-test-stop"
								} else {
									"voice-camera-test"
								}))
								.on_click(cx.listener(|this, _, _, cx| this.toggle_camera_test(cx))),
						),
					),
			)
			.child(
				self.field(
					"voice-video-quality",
					div()
						.w(theme.voice_field_width)
						.child(Select::new(&self.presets).w_full()),
					&theme,
					cx,
				),
			)
	}
}
