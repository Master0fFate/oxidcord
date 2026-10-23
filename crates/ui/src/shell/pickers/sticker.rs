use gpui::{
	AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ObjectFit, ParentElement as _,
	SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
};
use gpui_component::scroll::ScrollableElement as _;
use protocol::{Snowflake, Sticker};
use session::Command;

use super::{Picker, PickerEvent};
use crate::components::{Avatar, tooltip};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::shell::community::art::{cover, sticker_url};
use crate::theme::{Theme, rem};

const OTHER_GUILD_LOADS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
	Guild(Snowflake),
	Pack(Snowflake),
}

#[derive(Default)]
pub struct StickerState {
	section: Option<Section>,
	hovered: Option<Snowflake>,
}

struct SectionView {
	key: Section,
	name: SharedString,
	icon: Option<String>,
	guild_icon: Option<String>,
	stickers: Vec<Sticker>,
}

impl Picker {
	pub(super) fn load_stickers(&mut self, cx: &mut Context<Self>) {
		let (need_packs, guilds): (bool, Vec<Snowflake>) = {
			let data = self.data.read(cx);
			let state = data.state();
			let mut guilds: Vec<Snowflake> = self.guild.into_iter().collect();
			guilds.extend(
				state
					.guilds()
					.map(|guild| guild.id)
					.filter(|id| Some(*id) != self.guild)
					.take(OTHER_GUILD_LOADS),
			);
			guilds.retain(|id| state.guild_stickers(*id).is_none());
			(state.sticker_packs().is_none(), guilds)
		};
		if need_packs {
			self.send(Command::LoadStickerPacks, cx);
		}
		for guild_id in guilds {
			self.send(Command::LoadGuildStickers { guild_id }, cx);
		}
	}

	fn sections(&self, cx: &Context<Self>) -> Vec<SectionView> {
		let data = self.data.read(cx);
		let state = data.state();
		let mut order: Vec<Snowflake> = self.guild.into_iter().collect();
		order.extend(
			state
				.guilds()
				.map(|guild| guild.id)
				.filter(|id| Some(*id) != self.guild),
		);
		let mut sections: Vec<SectionView> = order
			.into_iter()
			.filter_map(|id| {
				let stickers = state.guild_stickers(id)?;
				if stickers.is_empty() {
					return None;
				}
				let guild = state.guild(id)?;
				Some(SectionView {
					key: Section::Guild(id),
					name: guild.name.clone().into(),
					icon: None,
					guild_icon: guild
						.icon
						.as_deref()
						.map(|hash| media::cdn::guild_icon_url(id.get(), hash, 64)),
					stickers: stickers.to_vec(),
				})
			})
			.collect();
		for pack in state.sticker_packs().unwrap_or_default() {
			let cover_sticker = pack
				.cover_sticker_id
				.and_then(|id| pack.stickers.iter().find(|sticker| sticker.id == id))
				.or_else(|| pack.stickers.first());
			sections.push(SectionView {
				key: Section::Pack(pack.id),
				name: pack.name.clone().into(),
				icon: cover_sticker.and_then(sticker_url),
				guild_icon: None,
				stickers: pack.stickers.to_vec(),
			});
		}
		sections
	}

	fn pick_sticker(&mut self, sticker: &Sticker, cx: &mut Context<Self>) {
		self.send(
			Command::SendSticker {
				channel_id: self.channel,
				sticker_id: sticker.id,
			},
			cx,
		);
		cx.emit(PickerEvent::Sent);
	}

	fn sticker_cell(&self, sticker: &Sticker, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let id = sticker.id;
		let hovered = self.sticker.hovered == Some(id);
		let picked = sticker.clone();
		let url = sticker_url(sticker);
		let cell = div()
			.id(("sticker", id.get()))
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.picker_sticker)
			.rounded(theme.radius_md)
			.when(hovered, |cell| cell.bg(theme.modifier_selected))
			.cursor_pointer()
			.on_hover(cx.listener(move |this, over: &bool, _, cx| {
				if *over {
					this.sticker.hovered = Some(id);
				} else if this.sticker.hovered == Some(id) {
					this.sticker.hovered = None;
				}
				cx.notify();
			}))
			.on_click(cx.listener(move |this, _, _, cx| this.pick_sticker(&picked, cx)));
		match url {
			Some(url) => cell
				.child(div().size(theme.picker_sticker - theme.space_8).child(cover(
					&url,
					theme.picker_sticker,
					theme.picker_sticker,
					ObjectFit::Contain,
					cx,
				)))
				.into_any_element(),
			None => cell
				.tooltip(tooltip(cx.t("picker-sticker-lottie")))
				.child(
					Icon::new(IconKind::Sticker)
						.size(theme.portrait_lg)
						.color(theme.interactive_muted),
				)
				.into_any_element(),
		}
	}

	fn rail_button(
		&self,
		index: usize,
		section: &SectionView,
		active: bool,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let key = section.key;
		let icon: AnyElement = match (&section.icon, &section.guild_icon) {
			(Some(url), _) | (None, Some(url)) => div()
				.size(theme.portrait_md)
				.rounded(theme.radius_sm)
				.overflow_hidden()
				.child(cover(
					url,
					theme.portrait_md,
					theme.portrait_md,
					ObjectFit::Cover,
					cx,
				))
				.into_any_element(),
			_ => Avatar::new(section.name.clone())
				.size(theme.portrait_md)
				.into_any_element(),
		};
		div()
			.id(("sticker-section", index))
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size(theme.picker_rail - theme.space_8)
			.rounded(theme.radius_sm)
			.cursor_pointer()
			.when(active, |button| button.bg(theme.modifier_selected))
			.when(!active, |button| {
				button.hover(|style| style.bg(theme.modifier_hover))
			})
			.tooltip(tooltip(section.name.clone()))
			.on_click(cx.listener(move |this, _, _, cx| {
				this.sticker.section = Some(key);
				cx.notify();
			}))
			.child(icon)
			.into_any_element()
	}

	pub(super) fn sticker_body(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
		let sections = self.sections(cx);
		let active = self
			.sticker
			.section
			.filter(|key| sections.iter().any(|section| section.key == *key))
			.or_else(|| sections.first().map(|section| section.key));
		let searching = !self.query.is_empty();
		let shown: Vec<(SharedString, Vec<Sticker>)> = if searching {
			let matches: Vec<Sticker> = sections
				.iter()
				.flat_map(|section| section.stickers.iter())
				.filter(|sticker| {
					sticker.name.to_lowercase().contains(&self.query)
						|| sticker
							.tags
							.as_deref()
							.is_some_and(|tags| tags.to_lowercase().contains(&self.query))
				})
				.cloned()
				.collect();
			vec![(cx.t("picker-results"), matches)]
		} else {
			sections
				.iter()
				.filter(|section| Some(section.key) == active)
				.map(|section| (section.name.clone(), section.stickers.clone()))
				.collect()
		};
		let rail: Vec<AnyElement> = sections
			.iter()
			.enumerate()
			.map(|(index, section)| {
				self.rail_button(
					index,
					section,
					!searching && Some(section.key) == active,
					theme,
					cx,
				)
			})
			.collect();
		let hovered = self.sticker.hovered.and_then(|id| {
			sections
				.iter()
				.flat_map(|section| section.stickers.iter())
				.find(|sticker| sticker.id == id)
		});
		let preview = hovered.map(|sticker| {
			(
				SharedString::from(sticker.name.clone()),
				SharedString::from(sticker.tags.clone().unwrap_or_default()),
			)
		});
		let groups: Vec<AnyElement> = shown
			.iter()
			.map(|(title, stickers)| {
				let cells: Vec<AnyElement> = stickers
					.iter()
					.map(|sticker| self.sticker_cell(sticker, theme, cx))
					.collect();
				div()
					.flex()
					.flex_col()
					.gap(theme.space_8)
					.child(
						div()
							.text_size(theme.font_tiny)
							.font_weight(FontWeight::BOLD)
							.text_color(theme.header_secondary)
							.child(title.to_uppercase()),
					)
					.child(div().flex().flex_wrap().gap(theme.gif_gap).children(cells))
					.when(stickers.is_empty(), |group| {
						group.child(
							div()
								.p(theme.space_32)
								.text_size(theme.font_sm)
								.text_color(theme.text_muted)
								.child(cx.t("picker-sticker-empty")),
						)
					})
					.into_any_element()
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(
				div()
					.flex()
					.flex_none()
					.gap(theme.space_4)
					.px(theme.space_12)
					.py(theme.space_8)
					.overflow_hidden()
					.children(rail),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.px(theme.space_12)
					.pb(theme.space_12)
					.children(groups)
					.overflow_y_scrollbar()
					.id("sticker-scroll")
					.flex_1()
					.min_h_0(),
			)
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap(theme.space_8)
					.h(rem(40.))
					.px(theme.space_12)
					.border_t_1()
					.border_color(theme.divider)
					.text_size(theme.font_sm)
					.children(preview.map(|(name, tags)| {
						div()
							.flex()
							.items_center()
							.gap(theme.space_8)
							.child(
								div()
									.font_weight(FontWeight::SEMIBOLD)
									.text_color(theme.header_primary)
									.child(name),
							)
							.child(div().text_color(theme.text_muted).child(tags))
					})),
			)
			.into_any_element()
	}
}
