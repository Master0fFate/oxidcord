use gpui::{
	AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement, ObjectFit,
	ParentElement as _, StatefulInteractiveElement as _, Styled as _, canvas, div, hsla,
	prelude::FluentBuilder as _,
};
use media::cdn::{ProxyFormat, attachment_proxy};
use media::providers::{self, Player};
use protocol::Embed;

use super::RowAction;
use super::body::BodyEnv;
use super::media::{fit, picture};
use crate::components::tooltip;
use crate::i18n::Translate as _;
use crate::icons::{Icon, IconKind};
use crate::playback::{MediaKey, Playback};
use crate::theme::{Theme, rem};

const VIDEO_MAX_WIDTH: f32 = 400.;
const VIDEO_MAX_HEIGHT: f32 = 300.;
const WIDGET_MAX_WIDTH: f32 = 400.;
const BUTTON: f32 = 40.;
const BUTTON_ICON: f32 = 20.;

fn element_id(prefix: &'static str, key: MediaKey) -> ElementId {
	ElementId::NamedInteger(prefix.into(), key.message ^ key.item.rotate_left(17))
}

fn thumbnail_url(embed: &Embed, width: f32, height: f32) -> Option<String> {
	let media = embed.thumbnail.as_ref()?;
	let url = media.proxy_url.as_deref().or(media.url.as_deref())?;
	Some(attachment_proxy(
		url,
		(width * 2.) as u32,
		(height * 2.) as u32,
		ProxyFormat::Webp,
	))
}

fn activate(key: MediaKey, player: &Player, link: Option<String>, env: &BodyEnv) -> AnyElement {
	let document = providers::frame_document(player);
	let dispatch = env.dispatch.clone();
	pill_button(element_id("embed-play", key), IconKind::Play, env.theme)
		.on_click(move |_, window, cx| {
			cx.stop_propagation();
			if cx
				.global::<Playback>()
				.activate_frame(key, document.clone(), window)
			{
				window.refresh();
			} else if let Some(link) = link.clone() {
				dispatch(RowAction::OpenLink(link), window, cx);
			}
		})
		.into_any_element()
}

fn pill_button(id: ElementId, icon: IconKind, theme: &Theme) -> gpui::Stateful<gpui::Div> {
	div()
		.id(id)
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size(rem(BUTTON))
		.rounded_full()
		.cursor_pointer()
		.hover(|style| style.bg(hsla(0., 0., 1., 0.12)))
		.child(Icon::new(icon).size(rem(BUTTON_ICON)).color(theme.text_on_brand))
}

fn live_frame(key: MediaKey, width: f32, height: f32, theme: &Theme) -> AnyElement {
	div()
		.relative()
		.mt(theme.space_4)
		.w(rem(width))
		.h(rem(height))
		.max_w_full()
		.rounded(theme.radius_sm)
		.overflow_hidden()
		.bg(hsla(0., 0., 0., 1.))
		.child(
			canvas(
				|_, _, _| {},
				move |bounds, (), window, cx| cx.global::<Playback>().paint_frame(key, bounds, window),
			)
			.absolute()
			.size_full(),
		)
		.into_any_element()
}

fn controls(key: MediaKey, player: &Player, embed: &Embed, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let link = embed.url.clone();
	let open = link.clone();
	let dispatch = env.dispatch.clone();
	div()
		.flex()
		.items_center()
		.gap(theme.space_4)
		.p(theme.space_4)
		.rounded_full()
		.bg(theme.overlay)
		.child(activate(key, player, link, env))
		.when_some(open, |pill, url| {
			pill.child(
				pill_button(element_id("embed-open", key), IconKind::ExternalLink, theme)
					.tooltip(tooltip(cx.t("embed-open-browser")))
					.on_click(move |_, window, cx| {
						cx.stop_propagation();
						dispatch(RowAction::OpenLink(url.clone()), window, cx);
					}),
			)
		})
		.into_any_element()
}

pub fn video(key: MediaKey, player: &Player, embed: &Embed, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let (width, height) = fit(
		player.width as f32,
		player.height as f32,
		VIDEO_MAX_WIDTH,
		VIDEO_MAX_HEIGHT,
	);
	if cx.global::<Playback>().frame_active(key) {
		return live_frame(key, width, height, theme);
	}
	let placeholder = embed
		.thumbnail
		.as_ref()
		.and_then(|media| media.placeholder.as_deref());
	let poster = match thumbnail_url(embed, width, height) {
		Some(url) => picture(&url, placeholder, width, height, ObjectFit::Cover, theme, cx),
		None => div().size_full().bg(hsla(0., 0., 0., 1.)).into_any_element(),
	};
	div()
		.relative()
		.mt(theme.space_4)
		.w(rem(width))
		.h(rem(height))
		.max_w_full()
		.rounded(theme.radius_sm)
		.overflow_hidden()
		.child(poster)
		.child(
			div()
				.absolute()
				.inset_0()
				.flex()
				.items_center()
				.justify_center()
				.child(controls(key, player, embed, env, cx)),
		)
		.into_any_element()
}

pub fn widget(key: MediaKey, player: &Player, embed: &Embed, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let (width, height) = (WIDGET_MAX_WIDTH.min(player.width as f32), player.height as f32);
	if cx.global::<Playback>().frame_active(key) {
		return live_frame(key, width, height, theme);
	}
	let side = height.min(width / 3.);
	let placeholder = embed
		.thumbnail
		.as_ref()
		.and_then(|media| media.placeholder.as_deref());
	let art = thumbnail_url(embed, side, side)
		.map(|url| picture(&url, placeholder, side, side, ObjectFit::Cover, theme, cx));
	let title = embed
		.title
		.clone()
		.or_else(|| embed.provider.as_ref().and_then(|provider| provider.name.clone()))
		.unwrap_or_default();
	div()
		.relative()
		.mt(theme.space_4)
		.flex()
		.items_center()
		.gap(theme.space_12)
		.w(rem(width))
		.h(rem(height))
		.max_w_full()
		.pr(theme.space_12)
		.rounded(theme.radius_md)
		.overflow_hidden()
		.bg(theme.background_tertiary)
		.children(art.map(|art| div().flex_none().size(rem(side)).child(art)))
		.child(
			div()
				.flex_1()
				.min_w_0()
				.truncate()
				.text_size(theme.font_md)
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(theme.header_primary)
				.child(title),
		)
		.child(controls(key, player, embed, env, cx))
		.into_any_element()
}
