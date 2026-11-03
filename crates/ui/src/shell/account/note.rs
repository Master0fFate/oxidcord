use gpui::{
	AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
	Window, div,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use protocol::Snowflake;
use session::Command;

use super::parts::section_label;
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::theme::Theme;

const NOTE_LIMIT: usize = 256;

pub struct NoteBox {
	data: Entity<Data>,
	user: Snowflake,
	input: Entity<TextareaState>,
	saved: String,
	_subscriptions: [Subscription; 2],
}

impl NoteBox {
	pub fn new(data: Entity<Data>, user: Snowflake, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let saved = data
			.read(cx)
			.state()
			.note(user)
			.map(str::to_owned)
			.unwrap_or_default();
		let placeholder = cx.t("profile-note-placeholder");
		let input = cx.new(|cx| {
			let mut state = TextareaState::new(window, cx)
				.placeholder(placeholder)
				.auto_grow(2, 5);
			state.set_value(saved.clone(), window, cx);
			state
		});
		let edits = cx.subscribe_in(
			&input,
			window,
			|this, input, event: &InputEvent, window, cx| match event {
				InputEvent::Change => {
					let value = input.read(cx).value();
					if value.chars().count() > NOTE_LIMIT {
						let capped: String = value.chars().take(NOTE_LIMIT).collect();
						input.update(cx, |state, cx| state.set_value(capped, window, cx));
					}
				}
				InputEvent::Blur => this.commit(cx),
				_ => {}
			},
		);
		let loads = cx.subscribe_in(&data, window, |this, _, event: &DataEvent, window, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.loaded.contains(this.user)
			{
				this.adopt(window, cx);
			}
		});
		Self {
			data,
			user,
			input,
			saved,
			_subscriptions: [edits, loads],
		}
	}

	pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
		self.input.update(cx, |state, cx| state.focus(window, cx));
	}

	fn commit(&mut self, cx: &mut Context<Self>) {
		let value = self.input.read(cx).value();
		let value = value.trim();
		if value == self.saved {
			return;
		}
		self.saved = value.to_owned();
		self.data.read(cx).send(Command::SetNote {
			user_id: self.user,
			note: self.saved.clone(),
		});
	}

	fn adopt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let incoming = self
			.data
			.read(cx)
			.state()
			.note(self.user)
			.map(str::to_owned)
			.unwrap_or_default();
		let current = self.input.read(cx).value();
		if incoming == self.saved || current.trim() != self.saved {
			return;
		}
		self.saved.clone_from(&incoming);
		self.input
			.update(cx, |state, cx| state.set_value(incoming, window, cx));
	}
}

impl Render for NoteBox {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_4)
			.child(section_label(cx.t("profile-note"), &theme))
			.child(Textarea::new(&self.input))
	}
}
