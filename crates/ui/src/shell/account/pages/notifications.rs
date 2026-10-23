use std::rc::Rc;

use gpui::{Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::switch::Switch;
use store::Settings;

use super::widgets::{heading, row};
use crate::i18n::Translate as _;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

pub struct NotificationsPage {
	backend: Rc<SettingsBackend>,
	test_note: Option<SharedString>,
}

impl NotificationsPage {
	pub fn new(backend: Rc<SettingsBackend>) -> Self {
		Self {
			backend,
			test_note: None,
		}
	}

	fn write(&mut self, apply: impl Fn(&mut Settings), cx: &mut Context<Self>) {
		(self.backend.write)(&apply);
		cx.notify();
	}

	fn send_test(&mut self, cx: &mut Context<Self>) {
		let sounds = (self.backend.read)().notifications.sounds;
		let notification = platform::notifications::Notification {
			title: cx.t("app-name").to_string(),
			body: cx.t("notifications-test-body").to_string(),
			icon_path: None,
			app_id: crate::APP_ID.to_owned(),
			sound: sounds,
		};
		let task = cx
			.background_executor()
			.spawn(async move { platform::notifications::show(&notification) });
		cx.spawn(async move |this, cx| {
			let outcome = task.await;
			this.update(cx, |this, cx| {
				this.test_note = Some(match outcome {
					Ok(()) => cx.t("notifications-test-sent"),
					Err(error) => {
						tracing::warn!(%error, "could not show the test notification");
						cx.t("notifications-test-failed")
					}
				});
				cx.notify();
			})
			.ok();
		})
		.detach();
	}
}

impl Render for NotificationsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let settings = (self.backend.read)().notifications;
		let test_note = self.test_note.clone();
		div()
			.flex()
			.flex_col()
			.child(heading(cx.t("notifications-desktop"), &theme))
			.child(row(
				cx.t("notifications-enable"),
				Some(cx.t("notifications-enable-note")),
				Switch::new("notifications-enabled")
					.checked(settings.enabled)
					.on_change(cx.listener(|this, checked: &bool, _, cx| {
						let checked = *checked;
						this.write(move |settings| settings.notifications.enabled = checked, cx);
					})),
				&theme,
			))
			.child(row(
				cx.t("notifications-sounds"),
				Some(cx.t("notifications-sounds-note")),
				Switch::new("notifications-sounds")
					.checked(settings.sounds)
					.on_change(cx.listener(|this, checked: &bool, _, cx| {
						let checked = *checked;
						this.write(move |settings| settings.notifications.sounds = checked, cx);
					})),
				&theme,
			))
			.child(row(
				cx.t("notifications-content"),
				Some(cx.t("notifications-content-note")),
				Switch::new("notifications-content")
					.checked(settings.show_content)
					.on_change(cx.listener(|this, checked: &bool, _, cx| {
						let checked = *checked;
						this.write(move |settings| settings.notifications.show_content = checked, cx);
					})),
				&theme,
			))
			.child(row(
				cx.t("notifications-test"),
				test_note,
				Button::new("notifications-test-button")
					.small()
					.label(cx.t("notifications-test-button"))
					.on_click(cx.listener(|this, _, _, cx| this.send_test(cx))),
				&theme,
			))
	}
}
