use std::rc::Rc;

use gpui::{Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div};
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::switch::Switch;
use store::Settings;

use super::DataDir;
use super::widgets::{heading, row};
use crate::i18n::Translate as _;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

#[cfg(target_os = "windows")]
const RENDERER: &str = "DirectX";
#[cfg(target_os = "macos")]
const RENDERER: &str = "Metal";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const RENDERER: &str = "Vulkan";

pub struct AdvancedPage {
	backend: Rc<SettingsBackend>,
	startup_error: Option<SharedString>,
}

impl AdvancedPage {
	pub fn new(backend: Rc<SettingsBackend>) -> Self {
		Self {
			backend,
			startup_error: None,
		}
	}

	fn write(&mut self, apply: impl Fn(&mut Settings), cx: &mut Context<Self>) {
		(self.backend.write)(&apply);
		cx.notify();
	}

	fn set_open_on_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
		match platform::autostart::set_enabled(enabled) {
			Ok(()) => {
				self.startup_error = None;
				self.write(move |settings| settings.startup.open_on_login = enabled, cx);
			}
			Err(error) => {
				tracing::warn!(%error, "could not change the startup setting");
				self.startup_error = Some(cx.t("advanced-startup-failed"));
				cx.notify();
			}
		}
	}
}

impl Render for AdvancedPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let settings = (self.backend.read)();
		let directory = cx.try_global::<DataDir>().map(|dir| dir.0.clone());
		let path: SharedString = directory
			.as_ref()
			.map(|dir| dir.display().to_string())
			.unwrap_or_default()
			.into();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_16)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("advanced-startup"), &theme))
					.child(row(
						cx.t("advanced-open-on-login"),
						Some(
							self.startup_error
								.clone()
								.unwrap_or_else(|| cx.t("advanced-open-on-login-note")),
						),
						Switch::new("advanced-open-on-login")
							.checked(settings.startup.open_on_login)
							.on_change(cx.listener(|this, checked: &bool, _, cx| {
								this.set_open_on_login(*checked, cx);
							})),
						&theme,
					))
					.child(row(
						cx.t("advanced-minimize-to-tray"),
						Some(cx.t("advanced-minimize-to-tray-note")),
						Switch::new("advanced-minimize-to-tray")
							.checked(settings.window.minimize_to_tray)
							.on_change(cx.listener(|this, checked: &bool, _, cx| {
								let checked = *checked;
								this.write(move |settings| settings.window.minimize_to_tray = checked, cx);
							})),
						&theme,
					)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("advanced-developer"), &theme))
					.child(row(
						cx.t("advanced-developer-mode"),
						Some(cx.t("advanced-developer-mode-note")),
						Switch::new("advanced-developer-mode")
							.checked(settings.developer.developer_mode)
							.on_change(cx.listener(|this, checked: &bool, _, cx| {
								let checked = *checked;
								this.write(move |settings| settings.developer.developer_mode = checked, cx);
							})),
						&theme,
					))
					.child(row(
						cx.t("advanced-renderer"),
						Some(cx.t("advanced-renderer-note")),
						div()
							.text_size(theme.font_md)
							.text_color(theme.text_muted)
							.child(RENDERER),
						&theme,
					)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.child(heading(cx.t("advanced-data"), &theme))
					.child(row(
						cx.t("advanced-data-folder"),
						Some(path),
						Button::new("advanced-open-folder")
							.small()
							.label(cx.t("advanced-open-folder"))
							.on_click(move |_, _, _| {
								if let Some(directory) = &directory
									&& let Err(error) = platform::open::reveal_path(directory)
								{
									tracing::warn!(%error, "could not open the data folder");
								}
							}),
						&theme,
					)),
			)
	}
}
