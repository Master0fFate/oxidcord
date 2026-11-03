use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::slider::{Slider, SliderState};
use protocol::{SOUND_NAME_MAX, SOUND_NAME_MIN, Snowflake, SoundboardSound};
use session::Command;

use crate::components::{dialog_footer, field};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const FULL_VOLUME: f32 = 100.;

pub struct EditSound {
	data: Entity<Data>,
	guild: Snowflake,
	sound: SoundboardSound,
	name: Entity<InputState>,
	volume: Entity<SliderState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for EditSound {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, guild: Snowflake, sound: SoundboardSound) {
	let view = cx.new(|cx| EditSound::new(data, guild, sound, window, cx));
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t("sounds-edit-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

impl EditSound {
	fn new(
		data: Entity<Data>,
		guild: Snowflake,
		sound: SoundboardSound,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let name = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(sound.name.clone(), window, cx);
			state
		});
		let volume = cx.new(|_| {
			SliderState::new()
				.min(0.)
				.max(FULL_VOLUME)
				.step(1.)
				.default_value((sound.volume * FULL_VOLUME).clamp(0., FULL_VOLUME))
		});
		let subscription = cx.subscribe_in(&name, window, |this, _, event: &InputEvent, _, cx| match event {
			InputEvent::PressEnter { .. } => this.submit(cx),
			InputEvent::Change => cx.notify(),
			_ => {}
		});
		Self {
			data,
			guild,
			sound,
			name,
			volume,
			_subscription: subscription,
		}
	}

	fn trimmed_name(&self, cx: &App) -> String {
		self.name.read(cx).value().trim().to_owned()
	}

	fn valid_name(&self, cx: &App) -> bool {
		(SOUND_NAME_MIN..=SOUND_NAME_MAX).contains(&self.trimmed_name(cx).chars().count())
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if !self.valid_name(cx) {
			return;
		}
		let name = Some(self.trimmed_name(cx)).filter(|name| *name != self.sound.name);
		let volume = (self.volume.read(cx).value().end().round() / FULL_VOLUME).clamp(0., 1.);
		let volume = ((volume - self.sound.volume).abs() > f32::EPSILON).then_some(volume);
		if name.is_some() || volume.is_some() {
			self.data.read(cx).send(Command::EditSound {
				guild_id: self.guild,
				sound_id: self.sound.sound_id,
				name,
				volume,
			});
		}
		cx.emit(Close);
	}
}

impl Render for EditSound {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let enabled = self.valid_name(cx);
		let weak_cancel = cx.weak_entity();
		let weak_confirm = cx.weak_entity();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.pt(theme.space_8)
			.child(field(cx.t("sounds-name"), &theme, Input::new(&self.name)))
			.child(field(cx.t("sounds-volume"), &theme, Slider::new(&self.volume)))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("sounds-edit-save"),
				false,
				enabled,
				&theme,
				move |_, cx| {
					weak_cancel.update(cx, |_, cx| cx.emit(Close)).ok();
				},
				move |_, cx| {
					weak_confirm.update(cx, |this, cx| this.submit(cx)).ok();
				},
			))
	}
}
