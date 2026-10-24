use std::rc::Rc;

use gpui::{
	AnyElement, App, Context, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
	Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::progress::Progress;
use gpui_component::switch::Switch;
use gpui_component::{Disableable as _, Sizable as _};

use super::service::{BETA_CHANNEL, STABLE_CHANNEL, UpdateHandle, UpdateService, UpdateStatus};
use crate::format::file_size;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::SettingsBackend;
use crate::theme::{Theme, rem};

pub struct UpdatesPage {
	service: Entity<UpdateService>,
	backend: Rc<SettingsBackend>,
}

impl UpdatesPage {
	pub fn new(backend: Rc<SettingsBackend>, cx: &mut Context<Self>) -> Self {
		let service = cx.global::<UpdateHandle>().0.clone();
		cx.observe(&service, |_, _, cx| cx.notify()).detach();
		Self { service, backend }
	}

	pub fn watch<T: 'static>(cx: &mut Context<T>) {
		let service = cx.global::<UpdateHandle>().0.clone();
		cx.observe(&service, |_, _, cx| cx.notify()).detach();
	}

	pub fn ready_badge(theme: &Theme, cx: &App) -> Option<AnyElement> {
		cx.global::<UpdateHandle>().0.read(cx).is_ready().then(|| {
			div()
				.ml_auto()
				.px(theme.space_8)
				.rounded_full()
				.bg(theme.brand)
				.text_size(theme.font_tiny)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.text_on_brand)
				.child(cx.t("updates-badge"))
				.into_any_element()
		})
	}

	pub fn title_prompt(theme: &Theme, cx: &App) -> Option<AnyElement> {
		let service = cx.global::<UpdateHandle>().0.clone();
		let (key, actionable) = match service.read(cx).status() {
			UpdateStatus::Available { .. } => ("updates-prompt-available", true),
			UpdateStatus::Downloading { .. } => ("updates-prompt-downloading", false),
			UpdateStatus::Ready { .. } => ("updates-prompt-ready", true),
			_ => return None,
		};
		Some(
			div()
				.id("update-prompt")
				.flex()
				.items_center()
				.gap(theme.space_4)
				.h(rem(16.))
				.mr(theme.space_8)
				.px(theme.space_8)
				.rounded_full()
				.bg(theme.success)
				.text_size(theme.font_tiny)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.text_on_brand)
				.when(actionable, |prompt| {
					prompt
						.cursor_pointer()
						.on_click(move |_, _, cx| service.update(cx, |service, cx| service.act(cx)))
				})
				.child(
					Icon::new(IconKind::Download)
						.size(rem(10.))
						.color(theme.text_on_brand),
				)
				.child(cx.t(key))
				.into_any_element(),
		)
	}

	fn summary(&self, cx: &App) -> SharedString {
		match self.service.read(cx).status() {
			UpdateStatus::Idle => cx.t("updates-idle"),
			UpdateStatus::Checking => cx.t("updates-checking"),
			UpdateStatus::UpToDate => cx.t("updates-up-to-date"),
			UpdateStatus::CouldNotCheck => cx.t("updates-could-not-check"),
			UpdateStatus::Available { version, .. } => cx.t_args(
				"updates-available",
				&[("version", ArgValue::from(version.as_str()))],
			),
			UpdateStatus::Downloading { version, .. } => cx.t_args(
				"updates-downloading",
				&[("version", ArgValue::from(version.as_str()))],
			),
			UpdateStatus::Ready { version } => {
				cx.t_args("updates-ready", &[("version", ArgValue::from(version.as_str()))])
			}
			UpdateStatus::Installing => cx.t("updates-installing"),
			UpdateStatus::Failed(error) => {
				cx.t_args("updates-failed", &[("error", ArgValue::from(error.as_str()))])
			}
		}
	}

	fn actions(&self, cx: &App) -> Vec<AnyElement> {
		let service = self.service.clone();
		let status = service.read(cx).status().clone();
		let busy = service.read(cx).is_busy();
		let mut actions = vec![
			Button::new("updates-check")
				.small()
				.label(cx.t("updates-check"))
				.disabled(busy)
				.on_click({
					let service = service.clone();
					move |_, _, cx| service.update(cx, |service, cx| service.check(cx))
				})
				.into_any_element(),
		];
		match status {
			UpdateStatus::Available {
				installable,
				notes_url,
				..
			} => {
				if installable {
					actions.push(
						Button::new("updates-download")
							.small()
							.primary()
							.label(cx.t("updates-download"))
							.on_click(move |_, _, cx| service.update(cx, |service, cx| service.download(cx)))
							.into_any_element(),
					);
				}
				actions.push(
					Button::new("updates-notes")
						.small()
						.label(cx.t("updates-notes"))
						.on_click(move |_, _, _| {
							if let Err(error) = platform::open::open_url(&notes_url) {
								tracing::warn!(%error, "could not open the release notes");
							}
						})
						.into_any_element(),
				);
			}
			UpdateStatus::Ready { .. } => actions.push(
				Button::new("updates-restart")
					.small()
					.primary()
					.label(cx.t("updates-restart"))
					.on_click(move |_, _, cx| service.update(cx, |service, cx| service.restart(cx)))
					.into_any_element(),
			),
			_ => {}
		}
		actions
	}

	fn detail(&self, theme: &Theme, cx: &App) -> Option<AnyElement> {
		match self.service.read(cx).status() {
			UpdateStatus::Downloading { done, total, .. } => {
				let percent = if *total == 0 {
					0.
				} else {
					*done as f32 / *total as f32 * 100.
				};
				Some(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_4)
						.child(Progress::new("updates-progress").value(percent))
						.child(
							div()
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(format!("{} / {}", file_size(*done), file_size(*total))),
						)
						.into_any_element(),
				)
			}
			UpdateStatus::Available {
				installable: false, ..
			} => Some(
				div()
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t("updates-managed"))
					.into_any_element(),
			),
			_ => None,
		}
	}
}

impl Render for UpdatesPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let info = updater::version_info();
		let current = match info.git_sha {
			Some(sha) => format!("{} ({sha})", info.version),
			None => info.version.to_owned(),
		};
		let updates = (self.backend.read)().updates;
		let check_on_start = updates.check_on_start;
		let development_branch = updates.channel == BETA_CHANNEL;
		let summary = self.summary(cx);
		let detail = self.detail(&theme, cx);
		let actions = self.actions(cx);
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_4)
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(cx.t("updates-current").to_uppercase()),
					)
					.child(
						div()
							.text_size(theme.font_md)
							.text_color(theme.header_primary)
							.child(current),
					),
			)
			.child(
				div()
					.text_size(theme.font_md)
					.text_color(theme.text_normal)
					.child(summary),
			)
			.children(detail)
			.child(div().flex().items_center().gap(theme.space_8).children(actions))
			.child(
				div().mt(rem(8.)).child(
					Switch::new("updates-check-on-start")
						.checked(check_on_start)
						.label(cx.t("updates-check-on-start"))
						.on_change(cx.listener(|this, checked: &bool, _, cx| {
							let checked = *checked;
							(this.backend.write)(&move |settings| settings.updates.check_on_start = checked);
							cx.notify();
						})),
				),
			)
			.child(
				Switch::new("updates-development-branch")
					.checked(development_branch)
					.label(cx.t("updates-development-branch"))
					.on_change(cx.listener(|this, checked: &bool, _, cx| {
						let channel = if *checked { BETA_CHANNEL } else { STABLE_CHANNEL };
						(this.backend.write)(&move |settings| settings.updates.channel = channel.to_owned());
						this.service.update(cx, |service, cx| service.check(cx));
						cx.notify();
					})),
			)
	}
}
