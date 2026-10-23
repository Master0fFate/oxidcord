use std::sync::Arc;

use ::login::{QrEvent, QrLogin};
use gpui::{
	AnyElement, Context, FontWeight, IntoElement, ObjectFit, ParentElement as _, RenderImage, SharedString,
	Styled as _, StyledImage as _, div, img, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use image::RgbaImage;

use super::LoginScreen;
use crate::components::Avatar;
use crate::i18n::{ArgValue, Translate as _};
use crate::images;
use crate::theme::Theme;

pub(super) enum QrState {
	Idle,
	Starting,
	Code(Arc<RenderImage>),
	Scanned { name: SharedString, avatar: String },
	Expired,
	Cancelled,
	Failed(String),
}

impl LoginScreen {
	pub(super) fn start_qr(&mut self, cx: &mut Context<Self>) {
		self.qr_state = QrState::Starting;
		let entered = self.runtime.enter();
		let mut session = QrLogin::start(self.options.qr_config.clone());
		drop(entered);
		let Some(mut events) = session.events() else {
			return;
		};
		self.qr_session = Some(session);
		self.qr_task = Some(cx.spawn(async move |this, cx| {
			while let Some(event) = events.recv().await {
				let finished = this
					.update(cx, |this, cx| this.on_qr_event(event, cx))
					.unwrap_or(true);
				if finished {
					break;
				}
			}
		}));
		cx.notify();
	}

	fn on_qr_event(&mut self, event: QrEvent, cx: &mut Context<Self>) -> bool {
		let finished = match event {
			QrEvent::Code { bitmap, .. } => {
				if let Some(image) =
					RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.rgba).map(images::from_rgba)
				{
					self.qr_state = QrState::Code(image);
				}
				false
			}
			QrEvent::Scanned { username, avatar, id } => {
				let avatar = media::cdn::avatar_url(id.parse().unwrap_or(0), avatar.as_deref(), None, 128);
				self.qr_state = QrState::Scanned {
					name: username.into(),
					avatar,
				};
				false
			}
			QrEvent::Token(token) => {
				self.validate(token.as_str().to_owned(), cx);
				true
			}
			QrEvent::Expired => {
				self.qr_state = QrState::Expired;
				true
			}
			QrEvent::Cancelled => {
				self.qr_state = QrState::Cancelled;
				true
			}
			QrEvent::Error(reason) => {
				self.qr_state = QrState::Failed(reason);
				true
			}
		};
		cx.notify();
		finished
	}

	fn qr_message(&self, cx: &Context<Self>) -> Option<(SharedString, bool)> {
		let danger = |text: SharedString| Some((text, true));
		match &self.qr_state {
			QrState::Idle => Some((cx.t("login-qr-locked"), false)),
			QrState::Starting => Some((cx.t("login-qr-starting"), false)),
			QrState::Expired => danger(cx.t("login-qr-expired")),
			QrState::Cancelled => danger(cx.t("login-qr-cancelled")),
			QrState::Failed(reason) => {
				danger(cx.t_args("login-qr-error", &[("reason", ArgValue::from(reason.clone()))]))
			}
			QrState::Code(_) | QrState::Scanned { .. } => None,
		}
	}

	pub(super) fn qr_column(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let panel = match &self.qr_state {
			QrState::Code(image) => div()
				.rounded(theme.radius_md)
				.overflow_hidden()
				.child(
					img(image.clone())
						.size(theme.login_qr_size)
						.object_fit(ObjectFit::Fill),
				)
				.into_any_element(),
			QrState::Scanned { name, avatar } => div()
				.flex()
				.flex_col()
				.items_center()
				.justify_center()
				.gap(theme.space_8)
				.size(theme.login_qr_size)
				.child(
					Avatar::new(name.clone())
						.size(theme.avatar_lg)
						.url(avatar.clone())
						.cutout(theme.background_secondary),
				)
				.child(
					div()
						.text_size(theme.font_md)
						.font_weight(FontWeight::BOLD)
						.text_color(theme.header_primary)
						.child(name.clone()),
				)
				.child(
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_muted)
						.child(cx.t("login-qr-scanned")),
				)
				.into_any_element(),
			_ => {
				let (text, failed) = self.qr_message(cx).unwrap_or_default();
				let retry = matches!(
					self.qr_state,
					QrState::Expired | QrState::Cancelled | QrState::Failed(_)
				);
				div()
					.flex()
					.flex_col()
					.items_center()
					.justify_center()
					.gap(theme.space_12)
					.size(theme.login_qr_size)
					.p(theme.space_8)
					.rounded(theme.radius_md)
					.bg(theme.background_tertiary)
					.text_size(theme.font_sm)
					.text_center()
					.text_color(if failed { theme.danger } else { theme.text_muted })
					.child(text)
					.when(retry, |panel| {
						panel.child(
							Button::new("login-qr-refresh")
								.primary()
								.small()
								.label(cx.t("login-qr-refresh"))
								.on_click(cx.listener(|this, _, _, cx| this.start_qr(cx))),
						)
					})
					.into_any_element()
			}
		};
		div()
			.flex()
			.flex_col()
			.flex_none()
			.items_center()
			.gap(theme.space_16)
			.w(theme.login_qr_column)
			.child(
				div()
					.text_size(theme.font_xl)
					.font_weight(FontWeight::BOLD)
					.text_center()
					.text_color(theme.header_primary)
					.child(cx.t("login-qr-title")),
			)
			.child(panel)
			.child(
				div()
					.text_size(theme.font_sm)
					.text_center()
					.text_color(theme.text_muted)
					.child(cx.t("login-qr-hint")),
			)
			.into_any_element()
	}
}
