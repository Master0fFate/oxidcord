use std::cell::Cell;

use gpui::{
	AnyElement, App, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement, ObjectFit,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, div,
	prelude::FluentBuilder as _,
};
use gpui_component::Disableable as _;
use gpui_component::button::Button;
use gpui_component::menu::DropdownMenu as _;
use markdown::parse;
use protocol::{
	Attachment, ButtonStyle, Component, ComponentType, GalleryItem, Message, PartialEmoji, SelectOption,
	SeparatorSpacing, Snowflake, UnfurledMedia,
};
use session::ComponentUse;

use super::body::{BodyEnv, render_body};
use super::media::{self, fit, picture, proxied_media};
use super::{ImageTarget, RowAction, point_media};
use crate::components::{ContextMenuBuilder, unicode_emoji};
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::images::{Images, frame_id};
use crate::model::role_color;
use crate::theme::{Theme, rem, with_alpha};

const CONTAINER_WIDTH: f32 = 600.;
const GALLERY_WIDTH: f32 = 520.;
const GALLERY_HEIGHT: f32 = 350.;
const GALLERY_GAP: f32 = 4.;
const THUMBNAIL_SIZE: f32 = 85.;
const SELECT_WIDTH: f32 = 400.;
const BUTTON_EMOJI: f32 = 18.;
const ACCENT_WIDTH: f32 = 4.;
const FILE_WIDTH: f32 = 432.;
const OPTION_EMOJI: f32 = 20.;
const OPTION_WIDTH: f32 = 360.;

pub fn render(message: &Message, env: &BodyEnv, cx: &App) -> Vec<AnyElement> {
	if message.components.is_empty() {
		return Vec::new();
	}
	let builder = Builder {
		message,
		env,
		theme: env.theme,
		cx,
		next_id: Cell::new(0),
	};
	message
		.components
		.iter()
		.filter_map(|component| builder.component(component))
		.collect()
}

struct Builder<'a> {
	message: &'a Message,
	env: &'a BodyEnv<'a>,
	theme: &'a Theme,
	cx: &'a App,
	next_id: Cell<usize>,
}

impl Builder<'_> {
	fn id(&self) -> ElementId {
		let index = self.next_id.get();
		self.next_id.set(index + 1);
		(
			SharedString::from(format!("component-{}", self.message.id)),
			index,
		)
			.into()
	}

	fn component(&self, component: &Component) -> Option<AnyElement> {
		match component.kind {
			ComponentType::ActionRow => Some(self.action_row(component)),
			ComponentType::Button => Some(self.button(component)),
			kind if kind.is_select() => Some(self.select(component)),
			ComponentType::Section => Some(self.section(component)),
			ComponentType::TextDisplay => component.content.as_deref().map(|text| self.text(text)),
			ComponentType::Thumbnail => component
				.media
				.as_ref()
				.map(|media| self.image(media, THUMBNAIL_SIZE, THUMBNAIL_SIZE, component.spoiler)),
			ComponentType::MediaGallery => Some(self.gallery(&component.items)),
			ComponentType::File => component.file.as_ref().map(|file| self.file(component, file)),
			ComponentType::Separator => Some(self.separator(component)),
			ComponentType::Container => Some(self.container(component)),
			_ => None,
		}
	}

	fn children(&self, component: &Component) -> Vec<AnyElement> {
		component
			.components
			.iter()
			.filter_map(|child| self.component(child))
			.collect()
	}

	fn text(&self, content: &str) -> AnyElement {
		div()
			.min_w_0()
			.child(render_body(&parse(content), self.env, self.cx))
			.into_any_element()
	}

	fn container(&self, component: &Component) -> AnyElement {
		let theme = self.theme;
		let accent = component.accent_color.map(role_color);
		let body = if let Some(index) = self.concealed(component.spoiler) {
			self.spoiler_cover(index, rem(120.))
		} else {
			div()
				.flex()
				.flex_col()
				.flex_1()
				.min_w_0()
				.gap(theme.space_8)
				.p(theme.space_16)
				.children(self.children(component))
				.into_any_element()
		};
		div()
			.flex()
			.mt(theme.space_4)
			.w_full()
			.max_w(rem(CONTAINER_WIDTH))
			.rounded(theme.radius_md)
			.overflow_hidden()
			.border_1()
			.border_color(theme.divider)
			.bg(theme.background_secondary)
			.children(accent.map(|accent| div().flex_none().w(rem(ACCENT_WIDTH)).bg(accent)))
			.child(body)
			.into_any_element()
	}

	fn section(&self, component: &Component) -> AnyElement {
		let accessory = component
			.accessory
			.as_deref()
			.and_then(|accessory| self.component(accessory));
		div()
			.flex()
			.items_start()
			.gap(self.theme.space_12)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap(self.theme.space_4)
					.children(self.children(component)),
			)
			.children(accessory.map(|accessory| div().flex_none().child(accessory)))
			.into_any_element()
	}

	fn separator(&self, component: &Component) -> AnyElement {
		let space = match component.spacing {
			Some(SeparatorSpacing::Large) => self.theme.space_8,
			_ => self.theme.space_2,
		};
		div()
			.py(space)
			.when(component.divider != Some(false), |separator| {
				separator.child(div().w_full().h(rem(1.)).bg(self.theme.divider))
			})
			.into_any_element()
	}

	fn action_row(&self, component: &Component) -> AnyElement {
		div()
			.flex()
			.flex_wrap()
			.gap(self.theme.space_8)
			.mt(self.theme.space_4)
			.children(self.children(component))
			.into_any_element()
	}

	fn usage(&self, component: &Component, values: Option<Vec<String>>) -> Option<RowAction> {
		let message = self.message;
		Some(RowAction::UseComponent(ComponentUse {
			guild_id: message.guild_id,
			channel_id: message.channel_id,
			message_id: message.id,
			message_flags: message.flags,
			application_id: message.application_id.unwrap_or(message.author.id),
			component_type: component.kind,
			custom_id: component.custom_id.clone()?,
			values,
		}))
	}

	fn emoji(&self, emoji: &PartialEmoji, size: f32) -> Option<AnyElement> {
		match (emoji.id, emoji.name.as_deref()) {
			(Some(id), _) => {
				let pixels = (size * 2.) as u32;
				let url = ::media::cdn::emoji_url(id.get(), emoji.animated, pixels);
				let image = self
					.cx
					.global::<Images>()
					.get(&url, ::media::SizeHint::square(pixels));
				Some(
					div()
						.flex_none()
						.size(rem(size))
						.when_some(image, |slot, image| {
							slot.child(gpui::img(image).id(frame_id(&url)).size_full())
						})
						.into_any_element(),
				)
			}
			(None, Some(name)) if !name.is_empty() => Some(unicode_emoji(name, rem(size), self.cx)),
			_ => None,
		}
	}

	fn button(&self, component: &Component) -> AnyElement {
		let theme = self.theme;
		let style = component.style.unwrap_or(ButtonStyle::Secondary);
		let (background, hover, foreground) = button_colors(style, theme);
		let action = match style {
			ButtonStyle::Link => component.url.clone().map(RowAction::OpenLink),
			ButtonStyle::Premium => None,
			_ => self.usage(component, None),
		};
		let enabled = !component.disabled && action.is_some();
		let dispatch = self.env.dispatch.clone();
		div()
			.id(self.id())
			.flex()
			.flex_none()
			.items_center()
			.gap(rem(6.))
			.h(rem(32.))
			.px(theme.space_16)
			.rounded(theme.radius_md)
			.bg(background)
			.text_size(theme.font_sm)
			.font_weight(FontWeight::MEDIUM)
			.text_color(foreground)
			.when(enabled, |button| {
				button
					.cursor_pointer()
					.hover(move |style| style.bg(hover))
					.on_click(move |_, window, cx| {
						if let Some(action) = action.clone() {
							dispatch(action, window, cx);
						}
					})
			})
			.when(!enabled, |button| button.opacity(0.5).cursor_not_allowed())
			.children(
				component
					.emoji
					.as_ref()
					.and_then(|emoji| self.emoji(emoji, BUTTON_EMOJI)),
			)
			.children(component.label.clone())
			.when(style == ButtonStyle::Link, |button| {
				button.child(Icon::new(IconKind::ExternalLink).size(rem(16.)).color(foreground))
			})
			.into_any_element()
	}

	fn select(&self, component: &Component) -> AnyElement {
		let theme = self.theme;
		let chosen: Vec<&str> = component
			.options
			.iter()
			.filter(|option| option.default)
			.map(|option| option.label.as_str())
			.collect();
		let shown: SharedString = if chosen.is_empty() {
			component
				.placeholder
				.clone()
				.unwrap_or_else(|| self.cx.t("component-select-placeholder").to_string())
				.into()
		} else {
			chosen.join(", ").into()
		};
		let enabled = !component.disabled
			&& component.kind == ComponentType::StringSelect
			&& !component.options.is_empty()
			&& component.custom_id.is_some();
		let menu = component
			.options
			.iter()
			.fold(ContextMenuBuilder::new(), |menu, option| {
				let action = self.usage(component, Some(vec![option.value.clone()]));
				let dispatch = self.env.dispatch.clone();
				menu.element_item(option_row(option), move |window, cx| {
					if let Some(action) = action.clone() {
						dispatch(action, window, cx);
					}
				})
			});
		let field = div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.w_full()
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_size(theme.font_sm)
					.text_color(if chosen.is_empty() {
						theme.text_muted
					} else {
						theme.text_normal
					})
					.child(shown),
			)
			.child(
				Icon::new(IconKind::ChevronDown)
					.size(rem(16.))
					.color(theme.interactive_normal),
			);
		div()
			.w_full()
			.max_w(rem(SELECT_WIDTH))
			.child(
				Button::new(self.id())
					.outline()
					.w_full()
					.h(rem(40.))
					.disabled(!enabled)
					.child(field)
					.dropdown_menu(menu.build()),
			)
			.into_any_element()
	}

	fn concealed(&self, spoiler: bool) -> Option<usize> {
		if !spoiler {
			return None;
		}
		let index = self.env.next_spoiler();
		(!self.env.is_revealed(index)).then_some(index)
	}

	fn spoiler_cover(&self, index: usize, height: gpui::Rems) -> AnyElement {
		let theme = self.theme;
		let message_id = self.env.message_id;
		let dispatch = self.env.dispatch.clone();
		div()
			.id(self.id())
			.flex()
			.flex_1()
			.items_center()
			.justify_center()
			.h(height)
			.bg(theme.spoiler)
			.cursor_pointer()
			.on_click(move |_, window, cx| dispatch(RowAction::ToggleSpoiler(message_id, index), window, cx))
			.child(
				div()
					.px(theme.space_12)
					.py(theme.space_4)
					.rounded_full()
					.bg(with_alpha(theme.background_tertiary, 0.8))
					.text_size(theme.font_tiny)
					.font_weight(FontWeight::BOLD)
					.text_color(theme.header_primary)
					.child(self.cx.t("component-spoiler").to_uppercase()),
			)
			.into_any_element()
	}

	fn image(&self, media: &UnfurledMedia, width: f32, height: f32, spoiler: bool) -> AnyElement {
		let theme = self.theme;
		if let Some(index) = self.concealed(spoiler) {
			return div()
				.flex()
				.w(rem(width))
				.h(rem(height))
				.rounded(theme.radius_md)
				.overflow_hidden()
				.child(self.spoiler_cover(index, rem(height)))
				.into_any_element();
		}
		let source = media.proxy_url.as_deref().unwrap_or(&media.url);
		let animated = media
			.content_type
			.as_deref()
			.is_some_and(|kind| kind == "image/gif");
		let shown = proxied_media(source, animated, width, height);
		let target = ImageTarget {
			url: media.url.clone(),
			name: file_name(&media.url),
			width: media.width.unwrap_or_default().max(0) as u32,
			height: media.height.unwrap_or_default().max(0) as u32,
		};
		let pointed = point_media(&target, source, None);
		let dispatch = self.env.dispatch.clone();
		div()
			.id(self.id())
			.w(rem(width))
			.h(rem(height))
			.rounded(theme.radius_md)
			.overflow_hidden()
			.cursor_pointer()
			.on_mouse_down(gpui::MouseButton::Right, pointed)
			.on_click(move |_, window, cx| dispatch(RowAction::OpenImage(target.clone()), window, cx))
			.child(picture(
				&shown,
				None,
				width,
				height,
				ObjectFit::Cover,
				theme,
				self.cx,
			))
			.into_any_element()
	}

	fn gallery(&self, items: &[GalleryItem]) -> AnyElement {
		let theme = self.theme;
		let shown: Vec<&GalleryItem> = items.iter().filter(|item| !item.media.url.is_empty()).collect();
		if let [item] = shown.as_slice() {
			if is_video(&item.media) {
				return self.attachment(&item.media, None, None);
			}
			let (width, height) = fit(
				item.media.width.unwrap_or(GALLERY_WIDTH as i64) as f32,
				item.media.height.unwrap_or(GALLERY_HEIGHT as i64) as f32,
				GALLERY_WIDTH,
				GALLERY_HEIGHT,
			);
			return div()
				.mt(theme.space_4)
				.child(self.image(&item.media, width, height, item.spoiler))
				.into_any_element();
		}
		let columns = if matches!(shown.len(), 2 | 4) { 2. } else { 3. };
		let cell = ((GALLERY_WIDTH - GALLERY_GAP * (columns - 1.)) / columns).floor();
		div()
			.flex()
			.flex_wrap()
			.gap(rem(GALLERY_GAP))
			.mt(theme.space_4)
			.w(rem(GALLERY_WIDTH))
			.max_w_full()
			.children(shown.iter().map(|item| {
				if is_video(&item.media) {
					self.video_tile(&item.media, cell)
				} else {
					self.image(&item.media, cell, cell, item.spoiler)
				}
			}))
			.into_any_element()
	}

	fn video_tile(&self, media: &UnfurledMedia, side: f32) -> AnyElement {
		let theme = self.theme;
		let poster = ::media::cdn::video_poster(&media.url, (side * 2.) as u32, (side * 2.) as u32);
		let url = media.url.clone();
		let dispatch = self.env.dispatch.clone();
		div()
			.id(self.id())
			.relative()
			.size(rem(side))
			.rounded(theme.radius_md)
			.overflow_hidden()
			.cursor_pointer()
			.bg(theme.background_tertiary)
			.on_click(move |_, window, cx| dispatch(RowAction::OpenLink(url.clone()), window, cx))
			.children(
				poster.map(|poster| picture(&poster, None, side, side, ObjectFit::Cover, theme, self.cx)),
			)
			.child(
				div()
					.absolute()
					.inset_0()
					.flex()
					.items_center()
					.justify_center()
					.child(
						div()
							.flex()
							.items_center()
							.justify_center()
							.size(rem(40.))
							.rounded_full()
							.bg(with_alpha(theme.background_tertiary, 0.8))
							.child(
								Icon::new(IconKind::Play)
									.size(rem(20.))
									.color(theme.header_primary),
							),
					),
			)
			.into_any_element()
	}

	fn file(&self, component: &Component, file: &UnfurledMedia) -> AnyElement {
		if let Some(index) = self.concealed(component.spoiler) {
			return div()
				.flex()
				.mt(self.theme.space_4)
				.w(rem(FILE_WIDTH))
				.max_w_full()
				.rounded(self.theme.radius_md)
				.overflow_hidden()
				.child(self.spoiler_cover(index, rem(64.)))
				.into_any_element();
		}
		self.attachment(file, component.name.as_deref(), component.size)
	}

	fn attachment(&self, file: &UnfurledMedia, name: Option<&str>, size: Option<i64>) -> AnyElement {
		let attached = self.message.attachments.iter().find(|attachment| {
			Some(attachment.id) == file.attachment_id || attachment.url.as_deref() == Some(file.url.as_str())
		});
		let attachment = attached.cloned().unwrap_or_else(|| Attachment {
			id: file.attachment_id.unwrap_or(Snowflake(self.next_id.get() as u64)),
			filename: name.map_or_else(|| file_name(&file.url), str::to_owned),
			content_type: file.content_type.clone(),
			size,
			url: Some(file.url.clone()),
			proxy_url: file.proxy_url.clone(),
			width: file.width,
			height: file.height,
			..Attachment::default()
		});
		div()
			.flex()
			.flex_col()
			.children(media::attachments(
				std::slice::from_ref(&attachment),
				self.env,
				self.env.prefs.inline_media,
				self.cx,
			))
			.into_any_element()
	}
}

fn option_row(option: &SelectOption) -> impl Fn(&mut gpui::Window, &mut App) -> AnyElement + 'static {
	let option = option.clone();
	move |_, cx| {
		let theme = cx.global::<Theme>();
		let emoji = option
			.emoji
			.as_ref()
			.and_then(|emoji| match (emoji.id, emoji.name.as_deref()) {
				(Some(id), _) => {
					let pixels = (OPTION_EMOJI * 2.) as u32;
					let url = ::media::cdn::emoji_url(id.get(), emoji.animated, pixels);
					let image = cx.global::<Images>().get(&url, ::media::SizeHint::square(pixels));
					Some(
						div()
							.flex_none()
							.size(rem(OPTION_EMOJI))
							.when_some(image, |slot, image| {
								slot.child(gpui::img(image).id(frame_id(&url)).size_full())
							})
							.into_any_element(),
					)
				}
				(None, Some(name)) if !name.is_empty() => Some(unicode_emoji(name, rem(OPTION_EMOJI), cx)),
				_ => None,
			});
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.min_w(rem(OPTION_WIDTH))
			.py(theme.space_4)
			.children(emoji)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.child(
						div()
							.truncate()
							.text_size(theme.font_sm)
							.font_weight(FontWeight::MEDIUM)
							.child(option.label.clone()),
					)
					.children(option.description.clone().map(|description| {
						div()
							.truncate()
							.text_size(theme.font_tiny)
							.text_color(theme.text_muted)
							.child(description)
					})),
			)
			.when(option.default, |row| {
				row.child(Icon::new(IconKind::Check).size(rem(16.)).color(theme.brand))
			})
			.into_any_element()
	}
}

fn button_colors(style: ButtonStyle, theme: &Theme) -> (Hsla, Hsla, Hsla) {
	match style {
		ButtonStyle::Primary => (theme.brand, theme.brand_hover, theme.text_on_brand),
		ButtonStyle::Success => (
			theme.success,
			with_alpha(theme.success, 0.85),
			theme.text_on_brand,
		),
		ButtonStyle::Danger => (theme.danger, theme.danger_hover, theme.text_on_brand),
		_ => (
			theme.button_secondary,
			theme.button_secondary_hover,
			theme.header_primary,
		),
	}
}

fn is_video(media: &UnfurledMedia) -> bool {
	media
		.content_type
		.as_deref()
		.is_some_and(|kind| kind.starts_with("video/"))
}

fn file_name(url: &str) -> String {
	url.split(['?', '#'])
		.next()
		.and_then(|path| path.rsplit('/').next())
		.filter(|name| !name.is_empty())
		.unwrap_or("file")
		.to_owned()
}
