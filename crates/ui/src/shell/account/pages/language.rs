use std::rc::Rc;

use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div};
use gpui_component::radio::{Radio, RadioGroup};

use super::widgets::heading;
use crate::i18n::{Localizer, Translate as _};
use crate::shell::SettingsBackend;
use crate::theme::Theme;

pub struct LanguagePage {
	backend: Rc<SettingsBackend>,
	locales: Vec<&'static str>,
}

impl LanguagePage {
	pub fn new(backend: Rc<SettingsBackend>) -> Self {
		Self {
			backend,
			locales: Localizer::available().collect(),
		}
	}

	fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
		let Some(locale) = self.locales.get(index).copied() else {
			return;
		};
		(self.backend.write)(&|settings| settings.locale = locale.to_owned());
		Localizer::new(locale).install(cx);
		cx.refresh_windows();
		cx.notify();
	}
}

impl Render for LanguagePage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let current = (self.backend.read)().locale;
		let selected = self
			.locales
			.iter()
			.position(|locale| *locale == current)
			.or_else(|| {
				let base = current.split('-').next().unwrap_or_default();
				self.locales
					.iter()
					.position(|locale| locale.split('-').next() == Some(base))
			});
		let labels: Vec<_> = self
			.locales
			.iter()
			.map(|locale| cx.t(&format!("language-{}", locale.to_lowercase())))
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(heading(cx.t("language-choose"), &theme))
			.child(
				RadioGroup::vertical("language-group")
					.selected_index(selected)
					.children(
						labels
							.into_iter()
							.enumerate()
							.map(|(index, label)| Radio::new(("language", index)).label(label)),
					)
					.on_change(cx.listener(|this, index: &usize, _, cx| this.choose(*index, cx))),
			)
			.child(
				div()
					.pt(theme.space_8)
					.text_size(theme.font_sm)
					.text_color(theme.text_muted)
					.child(cx.t("language-community-note")),
			)
	}
}
