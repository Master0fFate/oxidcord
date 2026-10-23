use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
	AnyElement, Context, Entity, EventEmitter, FontWeight, Hsla, IntoElement, ParentElement as _, Render,
	SharedString, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};

use super::widgets::{card, chip, note, section_label};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::shell::SettingsBackend;
use crate::theme::{Theme, rem};

const CARD_WIDTH: f32 = 220.;
const SWATCH_HEIGHT: f32 = 32.;
const BUILTIN: [(&str, &str); 3] = [
	("dark", "settings-theme-dark"),
	("light", "settings-theme-light"),
	("amoled", "settings-theme-amoled"),
];

pub struct ThemeApplied(pub String);

struct ThemeCard {
	name: String,
	label: SharedString,
	path: Option<PathBuf>,
	swatch: [Hsla; 6],
}

pub struct ThemesPage {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	cards: Vec<ThemeCard>,
	applied: String,
	_subscription: Subscription,
}

impl EventEmitter<ThemeApplied> for ThemesPage {}

fn swatch_of(theme: &Theme) -> [Hsla; 6] {
	[
		theme.background_tertiary,
		theme.background_secondary,
		theme.background_primary,
		theme.brand,
		theme.text_normal,
		theme.link,
	]
}

impl ThemesPage {
	pub fn new(data: Entity<Data>, backend: Rc<SettingsBackend>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |this, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.plugins
			{
				this.refresh(cx);
			}
		});
		let applied = (backend.read)().theme;
		let mut page = Self {
			data,
			backend,
			cards: Vec::new(),
			applied,
			_subscription: subscription,
		};
		page.refresh(cx);
		page
	}

	fn refresh(&mut self, cx: &mut Context<Self>) {
		let mut cards: Vec<ThemeCard> = BUILTIN
			.iter()
			.filter_map(|(name, label)| {
				Some(ThemeCard {
					name: (*name).to_owned(),
					label: cx.t(label),
					path: None,
					swatch: swatch_of(&Theme::builtin(name)?),
				})
			})
			.collect();
		let (files, plugins) = {
			let session = self.data.read(cx).session();
			(session.theme_files(), session.plugin_list())
		};
		for path in files {
			let Ok(theme) = Theme::from_file(&path) else {
				continue;
			};
			let Some(name) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
				continue;
			};
			cards.push(ThemeCard {
				label: plugins
					.iter()
					.find(|info| info.id == name)
					.map_or_else(|| name.clone(), |info| info.name.clone())
					.into(),
				name,
				swatch: swatch_of(&theme),
				path: Some(path),
			});
		}
		self.cards = cards;
		cx.notify();
	}

	fn apply(&mut self, index: usize, cx: &mut Context<Self>) {
		let Some(card) = self.cards.get(index) else {
			return;
		};
		let theme = match &card.path {
			Some(path) => Theme::from_file(path).ok(),
			None => Theme::builtin(&card.name),
		};
		let Some(theme) = theme else {
			return;
		};
		let name = card.name.clone();
		(self.backend.write)(&{
			let name = name.clone();
			move |settings| settings.theme.clone_from(&name)
		});
		theme.install(cx);
		self.applied.clone_from(&name);
		cx.emit(ThemeApplied(name));
		cx.notify();
	}

	fn open_folder(&self) {
		if let Err(error) = platform::open::reveal_path(&self.backend.themes_dir) {
			tracing::warn!(%error, "could not open the themes folder");
		}
	}

	fn theme_card(
		&self,
		index: usize,
		card_data: &ThemeCard,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let applied = self.applied == card_data.name;
		let strip = div()
			.flex()
			.h(rem(SWATCH_HEIGHT))
			.rounded(theme.radius_sm)
			.overflow_hidden()
			.children(
				card_data
					.swatch
					.iter()
					.map(|color| div().flex_1().h_full().bg(*color)),
			);
		card(theme)
			.w(rem(CARD_WIDTH))
			.child(strip)
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.gap(theme.space_8)
					.child(
						div()
							.min_w_0()
							.truncate()
							.text_size(theme.font_md)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.header_primary)
							.child(card_data.label.clone()),
					)
					.child(if applied {
						chip(cx.t("themes-applied"), theme.success, theme)
					} else {
						Button::new(("themes-apply", index))
							.small()
							.primary()
							.label(cx.t("themes-apply"))
							.on_click(cx.listener(move |this, _, _, cx| this.apply(index, cx)))
							.into_any_element()
					}),
			)
			.into_any_element()
	}
}

impl Render for ThemesPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let (builtin, plugin): (Vec<_>, Vec<_>) = self
			.cards
			.iter()
			.enumerate()
			.partition(|(_, card)| card.path.is_none());
		let builtin: Vec<AnyElement> = builtin
			.into_iter()
			.map(|(index, card)| self.theme_card(index, card, &theme, cx))
			.collect();
		let plugin: Vec<AnyElement> = plugin
			.into_iter()
			.map(|(index, card)| self.theme_card(index, card, &theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_24)
			.child(
				div().flex().child(
					Button::new("themes-open-folder")
						.small()
						.label(cx.t("themes-open-folder"))
						.on_click(cx.listener(|this, _, _, _| this.open_folder())),
				),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(section_label("themes-builtin", &theme, cx))
					.child(div().flex().flex_wrap().gap(theme.space_16).children(builtin)),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(section_label("themes-from-plugins", &theme, cx))
					.when(plugin.is_empty(), |section| {
						section.child(note(cx.t("themes-empty"), &theme))
					})
					.child(div().flex().flex_wrap().gap(theme.space_16).children(plugin)),
			)
	}
}
