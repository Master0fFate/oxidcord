use gpui::{
	App, AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
	Render, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use protocol::{Snowflake, Sticker};
use session::Command;

use crate::components::{dialog_footer, field};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::shell::community::modal::{self, Close};
use crate::theme::Theme;

const NAME_MIN: usize = 2;
const NAME_MAX: usize = 30;

pub struct EditSticker {
	data: Entity<Data>,
	guild: Snowflake,
	sticker: Sticker,
	name: Entity<InputState>,
	description: Entity<InputState>,
	tags: Entity<InputState>,
	_subscription: Subscription,
}

impl EventEmitter<Close> for EditSticker {}

pub fn open(window: &mut Window, cx: &mut App, data: Entity<Data>, guild: Snowflake, sticker: Sticker) {
	let view = cx.new(|cx| EditSticker::new(data, guild, sticker, window, cx));
	let focus = view.read(cx).name.read(cx).focus_handle(cx);
	let title = cx.t("stickers-edit-title");
	let width = cx.global::<Theme>().modal_width_sm;
	modal::open(window, cx, title, width, &view, Some(focus));
}

fn prefilled(value: &str, window: &mut Window, cx: &mut Context<EditSticker>) -> Entity<InputState> {
	cx.new(|cx| {
		let mut state = InputState::new(window, cx);
		state.set_value(value.to_owned(), window, cx);
		state
	})
}

impl EditSticker {
	fn new(
		data: Entity<Data>,
		guild: Snowflake,
		sticker: Sticker,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let name = prefilled(&sticker.name, window, cx);
		let description = prefilled(sticker.description.as_deref().unwrap_or_default(), window, cx);
		let tags = prefilled(sticker.tags.as_deref().unwrap_or_default(), window, cx);
		let subscription = cx.subscribe_in(&name, window, |this, _, event: &InputEvent, _, cx| match event {
			InputEvent::PressEnter { .. } => this.submit(cx),
			InputEvent::Change => cx.notify(),
			_ => {}
		});
		Self {
			data,
			guild,
			sticker,
			name,
			description,
			tags,
			_subscription: subscription,
		}
	}

	fn trimmed(input: &Entity<InputState>, cx: &App) -> String {
		input.read(cx).value().trim().to_owned()
	}

	fn valid_name(&self, cx: &App) -> bool {
		let length = Self::trimmed(&self.name, cx).chars().count();
		(NAME_MIN..=NAME_MAX).contains(&length)
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if !self.valid_name(cx) {
			return;
		}
		let changed = |input: &Entity<InputState>, current: Option<&str>| {
			let value = Self::trimmed(input, cx);
			(value != current.unwrap_or_default()).then_some(value)
		};
		let name = changed(&self.name, Some(&self.sticker.name));
		let description = changed(&self.description, self.sticker.description.as_deref());
		let tags = changed(&self.tags, self.sticker.tags.as_deref());
		if name.is_some() || description.is_some() || tags.is_some() {
			self.data.read(cx).send(Command::EditSticker {
				guild_id: self.guild,
				sticker_id: self.sticker.id,
				name,
				description,
				tags,
			});
		}
		cx.emit(Close);
	}
}

impl Render for EditSticker {
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
			.child(field(cx.t("stickers-name"), &theme, Input::new(&self.name)))
			.child(field(
				cx.t("stickers-description"),
				&theme,
				Input::new(&self.description),
			))
			.child(field(cx.t("stickers-tags"), &theme, Input::new(&self.tags)))
			.child(dialog_footer(
				cx.t("action-cancel"),
				cx.t("stickers-edit-save"),
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
