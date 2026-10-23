use gpui::{
	AnyElement, Context, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _, div,
	prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use session::Command;

use super::{AddOutcome, FriendsPage, SENT_FALLBACK};
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::Theme;

impl FriendsPage {
	pub(super) fn send_request(&mut self, cx: &mut Context<Self>) {
		let name = self.add_input.read(cx).value().trim().to_owned();
		if name.is_empty() || matches!(self.add_outcome, AddOutcome::Sending { .. }) {
			return;
		}
		let baseline = self
			.data
			.read(cx)
			.state()
			.last_rejection()
			.map_or(0, |rejection| rejection.id);
		self.add_outcome = AddOutcome::Sending {
			baseline,
			name: name.clone().into(),
		};
		self.data.read(cx).send(Command::AddFriend { username: name });
		cx.spawn(async move |this, cx| {
			cx.background_executor().timer(SENT_FALLBACK).await;
			this.update(cx, |this, cx| this.finish_sending(cx)).ok();
		})
		.detach();
		cx.notify();
	}

	pub(super) fn finish_sending(&mut self, cx: &mut Context<Self>) {
		if let AddOutcome::Sending { name, .. } = &self.add_outcome {
			self.add_outcome = AddOutcome::Sent(name.clone());
			cx.notify();
		}
	}

	pub(super) fn settle_rejection(&mut self, cx: &mut Context<Self>) {
		let AddOutcome::Sending { baseline, .. } = &self.add_outcome else {
			return;
		};
		let baseline = *baseline;
		let reason = {
			let data = self.data.read(cx);
			data.state()
				.last_rejection()
				.filter(|rejection| rejection.id > baseline)
				.map(|rejection| rejection.reason.clone())
		};
		if let Some(reason) = reason {
			self.add_outcome = AddOutcome::Failed(reason.into());
			cx.notify();
		}
	}

	pub(super) fn add_form(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let sending = matches!(self.add_outcome, AddOutcome::Sending { .. });
		let outcome: Option<(SharedString, bool)> = match &self.add_outcome {
			AddOutcome::Idle | AddOutcome::Sending { .. } => None,
			AddOutcome::Sent(name) => Some((
				cx.t_args("friends-add-sent", &[("name", ArgValue::from(name.to_string()))]),
				false,
			)),
			AddOutcome::Failed(reason) => Some((reason.clone(), true)),
		};
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.p(theme.space_24)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(
						div()
							.text_size(theme.font_md)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_primary)
							.child(cx.t("friends-add-title").to_uppercase()),
					)
					.child(
						div()
							.text_size(theme.font_sm)
							.text_color(theme.header_secondary)
							.child(cx.t("friends-add-hint")),
					),
			)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_12)
					.child(div().flex_1().child(Input::new(&self.add_input).large()))
					.child(
						Button::new("friends-add-send")
							.primary()
							.large()
							.loading(sending)
							.label(cx.t("friends-add-send"))
							.on_click(cx.listener(|this, _, _, cx| this.send_request(cx))),
					),
			)
			.children(outcome.map(|(text, failed)| {
				div()
					.text_size(theme.font_sm)
					.when(failed, |line| line.text_color(theme.danger))
					.when(!failed, |line| line.text_color(theme.success))
					.child(text)
			}))
			.into_any_element()
	}
}
