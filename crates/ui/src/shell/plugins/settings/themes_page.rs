use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
	AnyElement, AppContext as _, Context, Entity, EventEmitter, FontWeight, Hsla, IntoElement,
	ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
	prelude::FluentBuilder as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{Disableable as _, Sizable as _, WindowExt as _};
use plugin_host::catalog::ThemeEntry;
use session::Command;

use super::widgets::{banner, card, chip, note, section_label};
use crate::data::{Data, DataEvent};
use crate::i18n::{ArgValue, Translate as _};
use crate::shell::SettingsBackend;
use crate::theme::{Theme, rem};

const CARD_WIDTH: f32 = 220.;
const SWATCH_HEIGHT: f32 = 32.;
const URL_HEIGHT: f32 = 32.;
const DEFAULT_CATALOG_URL: &str =
	"https://raw.githubusercontent.com/Master0fFate/oxidcord/main/themes/catalog.json";
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
	from_plugin: bool,
	swatch: [Hsla; 6],
}

pub struct ThemesPage {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	cards: Vec<ThemeCard>,
	applied: String,
	installed: HashSet<String>,
	catalog_url: Entity<InputState>,
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
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let catalog_url = cx.new(|cx| {
			let mut state = InputState::new(window, cx);
			state.set_value(DEFAULT_CATALOG_URL, window, cx);
			state
		});
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
			installed: HashSet::new(),
			catalog_url,
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
					from_plugin: false,
					swatch: swatch_of(&Theme::builtin(name)?),
				})
			})
			.collect();
		let (files, plugins, catalog) = {
			let session = self.data.read(cx).session();
			(
				session.theme_files(),
				session.plugin_list(),
				session.theme_catalog(),
			)
		};
		let mut on_disk: Vec<PathBuf> = fs::read_dir(&self.backend.themes_dir)
			.into_iter()
			.flatten()
			.flatten()
			.map(|entry| entry.path())
			.filter(|path| path.extension().is_some_and(|extension| extension == "json"))
			.collect();
		on_disk.sort();
		self.installed = on_disk
			.iter()
			.filter_map(|path| path.file_stem())
			.map(|stem| stem.to_string_lossy().into_owned())
			.collect();
		let downloaded: Vec<PathBuf> = on_disk.into_iter().filter(|path| !files.contains(path)).collect();
		let sources = files
			.into_iter()
			.map(|path| (path, true))
			.chain(downloaded.into_iter().map(|path| (path, false)));
		for (path, from_plugin) in sources {
			let Ok(theme) = Theme::from_file(&path) else {
				continue;
			};
			let Some(name) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
				continue;
			};
			let label = plugins
				.iter()
				.find(|info| info.id == name)
				.map(|info| info.name.clone())
				.or_else(|| {
					catalog
						.iter()
						.flat_map(|catalog| catalog.themes.iter())
						.find(|entry| entry.id == name)
						.map(|entry| entry.name.clone())
				})
				.unwrap_or_else(|| name.clone());
			cards.push(ThemeCard {
				label: label.into(),
				name,
				swatch: swatch_of(&theme),
				path: Some(path),
				from_plugin,
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

	fn confirm_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let url = self.catalog_url.read(cx).value().trim().to_owned();
		if url.is_empty() {
			return;
		}
		let data = self.data.clone();
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			let url = url.clone();
			alert
				.title(cx.t("catalog-consent-title"))
				.description(cx.t_args("catalog-consent-body", &[("url", ArgValue::from(url.clone()))]))
				.confirm()
				.ok_text(cx.t("catalog-refresh"))
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx)
						.send(Command::ThemeCatalogRefresh { url: url.clone() });
					true
				})
		});
	}

	fn catalog_entry(
		&self,
		index: usize,
		entry: &ThemeEntry,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let present = self.installed.contains(&entry.id);
		let id = entry.id.clone();
		card(theme)
			.child(
				div()
					.flex()
					.items_start()
					.gap(theme.space_12)
					.child(
						div()
							.flex()
							.flex_col()
							.flex_1()
							.min_w_0()
							.gap(theme.space_2)
							.child(
								div()
									.flex()
									.items_center()
									.gap(theme.space_8)
									.child(
										div()
											.text_size(theme.font_md)
											.font_weight(FontWeight::SEMIBOLD)
											.text_color(theme.header_primary)
											.child(entry.name.clone()),
									)
									.child(
										div()
											.text_size(theme.font_sm)
											.text_color(theme.text_muted)
											.child(format!("v{}", entry.version)),
									),
							)
							.when(!entry.author.is_empty(), |column| {
								column.child(note(
									cx.t_args(
										"plugin-by",
										&[("author", ArgValue::from(entry.author.clone()))],
									),
									theme,
								))
							}),
					)
					.child(
						Button::new(("themes-catalog-install", index))
							.small()
							.primary()
							.label(if present {
								cx.t("catalog-installed")
							} else {
								cx.t("catalog-install")
							})
							.disabled(present)
							.on_click(cx.listener(move |this, _, _, cx| {
								this.data.read(cx).send(Command::ThemeInstall { id: id.clone() })
							})),
					),
			)
			.when(!entry.description.is_empty(), |card| {
				card.child(
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_normal)
						.child(entry.description.clone()),
				)
			})
			.into_any_element()
	}

	fn catalog_section(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let session = self.data.read(cx).session();
		let catalog = session.theme_catalog();
		let error = session.theme_catalog_error();
		let entries: Vec<AnyElement> = catalog
			.iter()
			.flat_map(|catalog| catalog.themes.iter())
			.enumerate()
			.map(|(index, entry)| self.catalog_entry(index, entry, theme, cx))
			.collect();
		div()
			.flex()
			.flex_col()
			.gap(theme.space_8)
			.child(section_label("themes-catalog", theme, cx))
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						div()
							.flex()
							.items_center()
							.flex_1()
							.min_w_0()
							.h(rem(URL_HEIGHT))
							.px(theme.space_8)
							.rounded(theme.radius_sm)
							.bg(theme.input_background)
							.child(
								Input::new(&self.catalog_url)
									.appearance(false)
									.bordered(false)
									.focus_bordered(false)
									.small()
									.w_full(),
							),
					)
					.child(
						Button::new("themes-catalog-refresh")
							.primary()
							.label(cx.t("catalog-refresh"))
							.on_click(cx.listener(|this, _, window, cx| this.confirm_refresh(window, cx))),
					),
			)
			.children(error.map(|error| banner(error, theme.danger, theme)))
			.when(entries.is_empty() && catalog.is_none(), |section| {
				section.child(note(cx.t("catalog-empty"), theme))
			})
			.child(div().flex().flex_col().gap(theme.space_8).children(entries))
			.into_any_element()
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
		let mut builtin = Vec::new();
		let mut plugin = Vec::new();
		let mut downloaded = Vec::new();
		for (index, card) in self.cards.iter().enumerate() {
			let element = self.theme_card(index, card, &theme, cx);
			if card.path.is_none() {
				builtin.push(element);
			} else if card.from_plugin {
				plugin.push(element);
			} else {
				downloaded.push(element);
			}
		}
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
			.when(!downloaded.is_empty(), |page| {
				page.child(
					div()
						.flex()
						.flex_col()
						.gap(theme.space_8)
						.child(section_label("themes-downloaded", &theme, cx))
						.child(div().flex().flex_wrap().gap(theme.space_16).children(downloaded)),
				)
			})
			.child(self.catalog_section(&theme, cx))
	}
}
