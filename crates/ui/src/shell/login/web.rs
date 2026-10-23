use std::sync::Arc;

use ::login::{LoginError, WebLogin};
use gpui::{AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div};
use gpui_component::button::Button;
use gpui_component::{Disableable as _, Sizable as _};

use super::LoginScreen;
use crate::i18n::{ArgValue, Translate as _};
use crate::theme::Theme;

pub(super) enum WebState {
	Idle,
	Waiting,
	Failed(String),
}

impl LoginScreen {
	fn start_web(&mut self, cx: &mut Context<Self>) {
		if !self.acknowledged || matches!(self.web_state, WebState::Waiting) {
			return;
		}
		let started = {
			let _entered = self.runtime.enter();
			match &self.options.web_helper {
				Some(helper) => WebLogin::start_with(helper),
				None => WebLogin::start(),
			}
		};
		let session = match started {
			Ok(session) => Arc::new(session),
			Err(error) => {
				self.web_state = WebState::Failed(error.to_string());
				cx.notify();
				return;
			}
		};
		self.web_run += 1;
		let run = self.web_run;
		self.web_state = WebState::Waiting;
		self.web_session = Some(session.clone());
		let (sender, receiver) = tokio::sync::oneshot::channel();
		self.runtime.spawn(async move {
			let _ = sender.send(session.wait().await);
		});
		cx.spawn(async move |this, cx| {
			let Ok(outcome) = receiver.await else {
				return;
			};
			this.update(cx, |this, cx| this.on_web_outcome(run, outcome, cx))
				.ok();
		})
		.detach();
		cx.notify();
	}

	fn on_web_outcome(
		&mut self,
		run: u64,
		outcome: Result<protocol::Token, LoginError>,
		cx: &mut Context<Self>,
	) {
		if run != self.web_run {
			return;
		}
		self.web_session = None;
		match outcome {
			Ok(token) => {
				self.web_state = WebState::Idle;
				self.validate(token.as_str().to_owned(), cx);
			}
			Err(LoginError::Cancelled) => self.web_state = WebState::Idle,
			Err(error) => self.web_state = WebState::Failed(error.to_string()),
		}
		cx.notify();
	}

	fn cancel_web(&mut self, cx: &mut Context<Self>) {
		self.web_run += 1;
		if let Some(session) = self.web_session.take() {
			session.abort();
		}
		self.web_state = WebState::Idle;
		cx.notify();
	}

	pub(super) fn web_controls(&self, locked: bool, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let message: Option<(SharedString, bool)> = match &self.web_state {
			WebState::Idle => None,
			WebState::Waiting => Some((cx.t("login-web-waiting"), false)),
			WebState::Failed(reason) => Some((
				cx.t_args("login-web-failed", &[("reason", ArgValue::from(reason.clone()))]),
				true,
			)),
		};
		let waiting = matches!(self.web_state, WebState::Waiting);
		let button = if waiting {
			Button::new("login-web-cancel")
				.large()
				.w_full()
				.label(cx.t("login-web-cancel"))
				.on_click(cx.listener(|this, _, _, cx| this.cancel_web(cx)))
		} else {
			Button::new("login-web-button")
				.large()
				.w_full()
				.disabled(locked)
				.label(cx.t("login-web"))
				.on_click(cx.listener(|this, _, _, cx| this.start_web(cx)))
		};
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(button)
			.children(message.map(|(text, failed)| {
				div()
					.text_size(theme.font_sm)
					.text_color(if failed { theme.danger } else { theme.text_muted })
					.child(text)
			}))
			.into_any_element()
	}
}
