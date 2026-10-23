use gpui::{
	AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _,
	SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::scroll::ScrollableElement as _;
use protocol::expressions::unicode::search_unicode;
use protocol::{EmojiCategory, Snowflake};
use state::{CatalogEmoji, EmojiCatalog, EmojiRef};

use super::{Picker, PickerEvent};
use crate::components::{Avatar, ContextMenuBuilder, tooltip};
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::shell::community::art::cover;
use crate::shell::emoji_picker::EmojiChoice;
use crate::theme::{Theme, rem};

const SEARCH_LIMIT: usize = 160;
const NITRO_URL: &str = "https://discord.com/nitro";
const CUSTOM_PIXELS: u32 = 64;
const TONES: [&str; 6] = [
	"",
	"\u{1F3FB}",
	"\u{1F3FC}",
	"\u{1F3FD}",
	"\u{1F3FE}",
	"\u{1F3FF}",
];
const TONE_SAMPLE: &str = "\u{1F44B}";
const SECTION_GLYPHS: [&str; 8] = [
	"\u{1F600}",
	"\u{1F43B}",
	"\u{1F354}",
	"\u{26BD}",
	"\u{1F697}",
	"\u{1F4A1}",
	"\u{2764}",
	"\u{1F3C1}",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
	Recent,
	Guild(Snowflake),
	Unicode(usize),
}

#[derive(Clone, PartialEq)]
struct Hover {
	label: SharedString,
	detail: Option<SharedString>,
	glyph: Option<String>,
	custom: Option<(u64, bool)>,
	locked: bool,
}

#[derive(Default)]
pub struct EmojiState {
	section: Option<Section>,
	hovered: Option<Hover>,
}

fn toned(glyph: &str, tone: usize) -> String {
	if tone == 0 {
		return glyph.to_owned();
	}
	let mut chars = glyph.chars();
	let Some(first) = chars.next() else {
		return String::new();
	};
	let mut out = String::with_capacity(glyph.len() + 4);
	out.push(first);
	out.push_str(TONES[tone]);
	let rest: String = chars.collect();
	out.push_str(rest.strip_prefix('\u{FE0F}').unwrap_or(&rest));
	out
}

impl Picker {
	fn custom_cell(
		&self,
		key: usize,
		emoji: &CatalogEmoji,
		guild_name: &SharedString,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let url = media::cdn::emoji_url(emoji.id.get(), emoji.animated, CUSTOM_PIXELS);
		let choice = EmojiChoice::Custom {
			name: emoji.name.clone(),
			id: emoji.id.get(),
			animated: emoji.animated,
		};
		let usable = emoji.usable;
		let hover = Hover {
			label: format!(":{}:", emoji.name).into(),
			detail: Some(guild_name.clone()),
			glyph: None,
			custom: Some((emoji.id.get(), emoji.animated)),
			locked: !emoji.usable,
		};
		let cell = div()
			.id(("emoji-custom", key))
			.relative()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.picker_cell)
			.rounded(theme.radius_sm)
			.on_hover(cx.listener(move |this, over: &bool, _, cx| {
				if *over {
					this.emoji.hovered = Some(hover.clone());
					cx.notify();
				}
			}))
			.child(
				div()
					.size(theme.picker_cell - theme.space_12)
					.when(!usable, |image| image.opacity(0.35))
					.child(cover(
						&url,
						theme.picker_cell,
						theme.picker_cell,
						ObjectFit::Contain,
						cx,
					)),
			);
		if usable {
			cell.cursor_pointer()
				.hover(|style| style.bg(theme.modifier_selected))
				.on_click(cx.listener(move |_, _, _, cx| cx.emit(PickerEvent::Emoji(choice.clone()))))
				.into_any_element()
		} else {
			cell.cursor_not_allowed()
				.tooltip(tooltip(cx.t("picker-emoji-locked")))
				.child(
					div().absolute().right(rem(2.)).bottom(rem(2.)).child(
						Icon::new(IconKind::Lock)
							.size(theme.glyph_sm - rem(4.))
							.color(theme.header_primary),
					),
				)
				.into_any_element()
		}
	}

	fn unicode_cell(
		&self,
		key: usize,
		glyph: &str,
		name: &str,
		capable: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let shown = if capable {
			toned(glyph, self.tone.get())
		} else {
			glyph.to_owned()
		};
		let choice = EmojiChoice::Unicode(shown.clone());
		let hover = Hover {
			label: format!(":{name}:").into(),
			detail: None,
			glyph: Some(shown.clone()),
			custom: None,
			locked: false,
		};
		div()
			.id(("emoji-unicode", key))
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.picker_cell)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.text_size(theme.font_xl)
			.hover(|style| style.bg(theme.modifier_selected))
			.on_hover(cx.listener(move |this, over: &bool, _, cx| {
				if *over {
					this.emoji.hovered = Some(hover.clone());
					cx.notify();
				}
			}))
			.on_click(cx.listener(move |_, _, _, cx| cx.emit(PickerEvent::Emoji(choice.clone()))))
			.child(shown)
			.into_any_element()
	}

	fn recent_cells(&self, catalog: &EmojiCatalog, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		let recent: Vec<EmojiRef> = self.data.read(cx).state().recent_emoji().to_vec();
		let mut cells = Vec::new();
		for (index, item) in recent.iter().enumerate() {
			match item.id {
				Some(id) => {
					let found = catalog
						.guilds
						.iter()
						.find_map(|section| {
							section
								.emojis
								.iter()
								.find(|emoji| emoji.id == id)
								.map(|emoji| (emoji.clone(), section.name.clone().into()))
						})
						.unwrap_or_else(|| {
							(
								CatalogEmoji {
									id,
									name: item.name.clone(),
									animated: item.animated,
									available: true,
									guild_id: Snowflake(0),
									external: true,
									usable: false,
								},
								SharedString::default(),
							)
						});
					cells.push(self.custom_cell(10_000 + index, &found.0, &found.1, theme, cx));
				}
				None => {
					cells.push(self.unicode_cell(10_000 + index, &item.name, &item.name, false, theme, cx));
				}
			}
		}
		cells
	}

	fn search_cells(&self, catalog: &EmojiCatalog, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
		let mut cells = Vec::new();
		let mut key = 0usize;
		for section in &catalog.guilds {
			let guild_name: SharedString = section.name.clone().into();
			for emoji in section
				.emojis
				.iter()
				.filter(|emoji| emoji.name.to_lowercase().contains(&self.query))
			{
				cells.push(self.custom_cell(key, emoji, &guild_name, theme, cx));
				key += 1;
			}
		}
		for entry in search_unicode(&self.query).into_iter().take(SEARCH_LIMIT) {
			cells.push(self.unicode_cell(
				key,
				entry.emoji,
				entry.shortcode,
				entry.skin_tone_capable,
				theme,
				cx,
			));
			key += 1;
		}
		cells
	}

	fn rail_item(
		&self,
		id: usize,
		section: Section,
		active: bool,
		(label, content): (SharedString, AnyElement),
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		div()
			.id(("emoji-section", id))
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.picker_rail - theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.when(active, |item| item.bg(theme.modifier_selected))
			.when(!active, |item| item.hover(|style| style.bg(theme.modifier_hover)))
			.tooltip(tooltip(label))
			.on_click(cx.listener(move |this, _, _, cx| {
				this.emoji.section = Some(section);
				this.query.clear();
				cx.notify();
			}))
			.child(content)
			.into_any_element()
	}

	fn tone_button(&self, cx: &mut Context<Self>) -> AnyElement {
		let weak = cx.weak_entity();
		let current = self.tone.get();
		let menu = TONES
			.iter()
			.enumerate()
			.fold(ContextMenuBuilder::new(), |menu, (index, tone)| {
				let weak = weak.clone();
				let sample = format!("{TONE_SAMPLE}{tone}");
				menu.checked_item(sample, index == current, move |_, cx| {
					weak.update(cx, |this, cx| {
						this.tone.set(index);
						cx.notify();
					})
					.ok();
				})
			});
		Button::new("emoji-tone")
			.ghost()
			.small()
			.label(format!("{TONE_SAMPLE}{}", TONES[current]))
			.tooltip(cx.t("picker-emoji-tone"))
			.dropdown_menu(menu.build())
			.into_any_element()
	}

	pub(super) fn emoji_body(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let catalog = self.data.read(cx).state().emoji_catalog(self.guild);
		let has_recent = !self.data.read(cx).state().recent_emoji().is_empty();
		let default = if has_recent {
			Section::Recent
		} else if let Some(first) = catalog.guilds.first() {
			Section::Guild(first.guild_id)
		} else {
			Section::Unicode(0)
		};
		let active = self.emoji.section.unwrap_or(default);
		let searching = !self.query.is_empty();
		let (title, cells): (SharedString, Vec<AnyElement>) = if searching {
			(cx.t("picker-results"), self.search_cells(&catalog, theme, cx))
		} else {
			match active {
				Section::Recent => (
					cx.t("picker-emoji-recent"),
					self.recent_cells(&catalog, theme, cx),
				),
				Section::Guild(id) => {
					let section = catalog.guilds.iter().find(|section| section.guild_id == id);
					let name: SharedString = section
						.map(|section| section.name.clone())
						.unwrap_or_default()
						.into();
					let cells = section
						.map(|section| {
							section
								.emojis
								.iter()
								.enumerate()
								.map(|(index, emoji)| self.custom_cell(index, emoji, &name, theme, cx))
								.collect()
						})
						.unwrap_or_default();
					(name, cells)
				}
				Section::Unicode(index) => {
					let section = catalog.unicode.get(index);
					let title = section.map_or_else(SharedString::default, |section| {
						cx.t(&format!("picker-emoji-{}", section.category.name()))
					});
					let cells = section
						.map(|section| {
							section
								.emojis
								.iter()
								.enumerate()
								.map(|(index, entry)| {
									self.unicode_cell(
										index,
										entry.emoji,
										entry.shortcode,
										entry.skin_tone_capable,
										theme,
										cx,
									)
								})
								.collect()
						})
						.unwrap_or_default();
					(title, cells)
				}
			}
		};
		let mut rail: Vec<AnyElement> = Vec::new();
		if has_recent {
			rail.push(
				self.rail_item(
					0,
					Section::Recent,
					!searching && active == Section::Recent,
					(
						cx.t("picker-emoji-recent"),
						Icon::new(IconKind::Clock)
							.size(theme.glyph_md)
							.color(theme.interactive_normal)
							.into_any_element(),
					),
					theme,
					cx,
				),
			);
		}
		for (index, section) in catalog.guilds.iter().enumerate() {
			let icon = section
				.icon
				.as_deref()
				.map(|hash| media::cdn::guild_icon_url(section.guild_id.get(), hash, 64));
			let avatar = Avatar::new(section.name.clone()).size(theme.portrait_md);
			let content = match icon {
				Some(url) => avatar.url(url).into_any_element(),
				None => avatar.into_any_element(),
			};
			rail.push(self.rail_item(
				1 + index,
				Section::Guild(section.guild_id),
				!searching && active == Section::Guild(section.guild_id),
				(section.name.clone().into(), content),
				theme,
				cx,
			));
		}
		for (index, section) in catalog.unicode.iter().enumerate() {
			let name = section.category.name();
			let glyph = SECTION_GLYPHS[EmojiCategory::ALL
				.iter()
				.position(|category| *category == section.category)
				.unwrap_or(0)];
			rail.push(self.rail_item(
				100 + index,
				Section::Unicode(index),
				!searching && active == Section::Unicode(index),
				(
					cx.t(&format!("picker-emoji-{name}")),
					div().text_size(theme.font_lg).child(glyph).into_any_element(),
				),
				theme,
				cx,
			));
		}
		let hovered = self.emoji.hovered.clone();
		let preview = hovered.map(|hover| {
			let visual: AnyElement = match (&hover.glyph, hover.custom) {
				(Some(glyph), _) => div()
					.text_size(theme.font_xl)
					.child(glyph.clone())
					.into_any_element(),
				(None, Some((id, animated))) => div()
					.size(theme.glyph_lg)
					.child(cover(
						&media::cdn::emoji_url(id, animated, CUSTOM_PIXELS),
						theme.glyph_lg,
						theme.glyph_lg,
						ObjectFit::Contain,
						cx,
					))
					.into_any_element(),
				(None, None) => div().into_any_element(),
			};
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(visual)
				.child(
					div()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(theme.header_primary)
						.child(hover.label.clone()),
				)
				.children(hover.detail.clone().map(|detail| {
					div()
						.text_size(theme.font_sm)
						.text_color(theme.text_muted)
						.child(cx.t_args(
							"picker-emoji-from",
							&[("name", ArgValue::from(detail.to_string()))],
						))
				}))
				.when(hover.locked, |row| {
					row.child(
						div()
							.id("emoji-get-nitro")
							.cursor_pointer()
							.text_size(theme.font_sm)
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(theme.link)
							.hover(|style| style.underline())
							.on_click(|_, _, _| {
								if let Err(error) = platform::open::open_url(NITRO_URL) {
									tracing::warn!(%error, "could not open the nitro page");
								}
							})
							.child(cx.t("picker-emoji-get-nitro")),
					)
				})
		});
		div()
			.flex()
			.flex_1()
			.min_h_0()
			.child(
				div()
					.flex()
					.flex_col()
					.flex_none()
					.items_center()
					.gap(theme.space_2)
					.w(theme.picker_rail)
					.py(theme.space_8)
					.border_r_1()
					.border_color(theme.divider)
					.children(rail)
					.overflow_y_scrollbar()
					.id("emoji-rail")
					.h_full(),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.child(
						div()
							.px(theme.space_12)
							.py(theme.space_8)
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(title.to_uppercase()),
					)
					.child(
						div()
							.flex()
							.flex_wrap()
							.content_start()
							.px(theme.space_8)
							.pb(theme.space_8)
							.children(cells)
							.overflow_y_scrollbar()
							.id("emoji-grid")
							.flex_1()
							.min_h_0(),
					)
					.child(
						div()
							.flex()
							.flex_none()
							.items_center()
							.justify_between()
							.h(theme.control_height + theme.space_8)
							.px(theme.space_12)
							.border_t_1()
							.border_color(theme.divider)
							.child(div().children(preview))
							.child(self.tone_button(cx)),
					),
			)
			.into_any_element()
	}
}
