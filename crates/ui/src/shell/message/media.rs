use chrono::Utc;
use gpui::{
	AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, MouseButton, ObjectFit,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, div,
	img, prelude::FluentBuilder as _,
};
use gpui_component::Sizable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use markdown::parse;
use media::SizeHint;
use media::cdn::{
	Playable, ProxyFormat, animated_proxy, attachment_proxy, is_gif, playable, sticker_url, video_poster,
};
use media::providers::{self, Candidate, Layout};
use plugin_host::ViewContext;
use plugin_host::api::model::{AttachmentLayout, AttachmentOverride, ClickTarget, EmojiRef};
use protocol::{
	Attachment, Embed, EmbedKind, EmbedMedia, Message, PartialEmoji, Poll, Reaction, Snowflake, StickerItem,
};

use super::body::{BodyEnv, render_body};
use super::player::{self, AudioSource, FileInfo, LoopSource, VideoSource, VoiceSource};
use super::provider;
use super::reaction_tip;
use super::{ImageTarget, RowAction, point_link, point_media, with_attachment_menu, with_link_menu};
use crate::components::{IconButton, tooltip, unicode_emoji};
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::images::{Images, frame_id};
use crate::model::role_color;
use crate::playback::MediaKey;
use crate::shell::plugins::{
	INLINE, MenuTarget, PatchEffects, contextual_inline, message_click_at, plugin_menu, watching_messages,
};
use crate::theme::{Theme, rem, with_alpha};

const MAX_IMAGE_WIDTH: f32 = 550.;
const MAX_IMAGE_HEIGHT: f32 = 350.;
const EMBED_WIDTH: f32 = 520.;
const EMBED_IMAGE_WIDTH: f32 = 400.;
const EMBED_IMAGE_HEIGHT: f32 = 300.;
const THUMBNAIL: f32 = 80.;
const FALLBACK_WIDTH: f32 = 300.;
const FALLBACK_HEIGHT: f32 = 200.;
const STICKER_SIZE: f32 = 160.;
const FIELD_WIDTH: f32 = 242.;
const PLACEHOLDER_SIDE: u32 = 32;
const REACTION_EMOJI: f32 = 16.;
const ANIMATED_FLAG: u64 = 1 << 5;
const EMBED_ITEM: u64 = 1 << 62;
const SPOILER_FLAG: u64 = 1 << 3;
const SPOILER_ITEM: usize = 1 << 40;
const SPOILER_PREFIX: &str = "SPOILER_";
const STACKED_MAX_HEIGHT: f32 = 720.;
const BLUR_DIVISOR: f32 = 24.;
const ATTACHMENT_ACTION_NODES: usize = 4;

#[derive(Clone, Copy)]
struct Look {
	stacked: bool,
}

fn sized(look: Look, width: f32, height: f32) -> (f32, f32) {
	if !look.stacked {
		return fit(width, height, MAX_IMAGE_WIDTH, MAX_IMAGE_HEIGHT);
	}
	let scale = (MAX_IMAGE_WIDTH / width).min(STACKED_MAX_HEIGHT / height);
	((width * scale).round().max(1.), (height * scale).round().max(1.))
}

pub fn fit(width: f32, height: f32, max_width: f32, max_height: f32) -> (f32, f32) {
	let scale = (max_width / width).min(max_height / height).min(1.);
	((width * scale).round().max(1.), (height * scale).round().max(1.))
}

fn dimensions(width: Option<i64>, height: Option<i64>) -> (f32, f32) {
	match (width, height) {
		(Some(width), Some(height)) if width > 0 && height > 0 => (width as f32, height as f32),
		_ => (FALLBACK_WIDTH, FALLBACK_HEIGHT),
	}
}

pub(super) fn picture(
	url: &str,
	placeholder: Option<&str>,
	width: f32,
	height: f32,
	fit_mode: ObjectFit,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	let images = cx.global::<Images>();
	let hint = SizeHint::new((width * 2.) as u32, (height * 2.) as u32);
	let shown = images
		.get(url, hint)
		.map(|image| {
			img(image)
				.id(frame_id(url))
				.size_full()
				.object_fit(fit_mode)
				.into_any_element()
		})
		.or_else(|| {
			let hash = placeholder?;
			let image = images.placeholder(hash, PLACEHOLDER_SIDE, PLACEHOLDER_SIDE)?;
			Some(
				img(image)
					.size_full()
					.object_fit(ObjectFit::Fill)
					.into_any_element(),
			)
		});
	div()
		.size_full()
		.bg(theme.background_secondary)
		.children(shown)
		.into_any_element()
}

fn proxied(url: &str, width: f32, height: f32) -> String {
	attachment_proxy(url, (width * 2.) as u32, (height * 2.) as u32, ProxyFormat::Webp)
}

pub(super) fn proxied_media(url: &str, animated: bool, width: f32, height: f32) -> String {
	if animated {
		animated_proxy(url, (width * 2.) as u32, (height * 2.) as u32)
	} else {
		proxied(url, width, height)
	}
}

fn attachment_animated(attachment: &Attachment) -> bool {
	attachment.flags.is_some_and(|flags| flags & ANIMATED_FLAG != 0)
		|| is_gif(attachment.content_type.as_deref(), &attachment.filename)
}

fn embed_animated(media: &EmbedMedia) -> bool {
	media.flags.is_some_and(|flags| flags & ANIMATED_FLAG != 0)
		|| media
			.url
			.as_deref()
			.is_some_and(|url| is_gif(media.content_type.as_deref(), url))
}

fn embed_key(env: &BodyEnv, index: usize) -> MediaKey {
	MediaKey {
		message: env.message_id,
		item: EMBED_ITEM | index as u64,
	}
}

fn is_image(attachment: &Attachment) -> bool {
	if let Some(kind) = &attachment.content_type {
		return kind.starts_with("image/");
	}
	let name = attachment.filename.to_ascii_lowercase();
	["png", "jpg", "jpeg", "gif", "webp"]
		.iter()
		.any(|extension| name.ends_with(&format!(".{extension}")))
}

fn is_spoiler(attachment: &Attachment) -> bool {
	attachment.filename.starts_with(SPOILER_PREFIX)
		|| attachment.flags.is_some_and(|flags| flags & SPOILER_FLAG != 0)
}

fn veiled(content: AnyElement, index: usize, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let dispatch = env.dispatch.clone();
	let message = env.message_id;
	div()
		.relative()
		.self_start()
		.child(content)
		.child(
			div()
				.id(("attachment-veil", index))
				.absolute()
				.top(theme.space_4)
				.left_0()
				.right_0()
				.bottom_0()
				.flex()
				.items_center()
				.justify_center()
				.rounded(theme.radius_md)
				.bg(theme.background_tertiary)
				.occlude()
				.cursor_pointer()
				.on_click(move |_, window, cx| {
					cx.stop_propagation();
					dispatch(
						RowAction::ToggleSpoiler(message, SPOILER_ITEM | index),
						window,
						cx,
					)
				})
				.child(
					div()
						.px(theme.space_12)
						.py(theme.space_4)
						.rounded_full()
						.bg(theme.background_floating)
						.text_size(theme.font_sm)
						.font_weight(FontWeight::BOLD)
						.text_color(theme.header_primary)
						.child(cx.t("attachment-spoiler-tag")),
				),
		)
		.into_any_element()
}

pub fn attachments(items: &[Attachment], env: &BodyEnv, inline: bool, cx: &App) -> Vec<AnyElement> {
	let layout = env.effects().and_then(PatchEffects::layout);
	let look = Look {
		stacked: layout == Some(AttachmentLayout::Stacked),
	};
	items
		.iter()
		.enumerate()
		.map(|(index, attachment)| {
			let kind = playable(attachment.content_type.as_deref(), &attachment.filename);
			let inline =
				inline || (layout == Some(AttachmentLayout::InlineVideo) && kind == Some(Playable::Video));
			let content = match (inline, kind) {
				(true, _) if is_image(attachment) => image_attachment(attachment, look, env, cx),
				(true, Some(Playable::Video)) => video_attachment(attachment, look, env, cx),
				(true, Some(Playable::Audio)) => audio_attachment(attachment, env, cx),
				_ => file_card(attachment, kind == Some(Playable::Video), env, cx),
			};
			let content = match env
				.effects()
				.and_then(|effects| effects.attachment_override(attachment.id))
			{
				Some(over) => customized(content, attachment, over, look, env, cx),
				None => content,
			};
			let hidden = is_spoiler(attachment) && !env.is_revealed(SPOILER_ITEM | index);
			let content = if hidden {
				veiled(content, index, env, cx)
			} else {
				content
			};
			with_actions(content, attachment, env, cx)
		})
		.collect()
}

fn with_actions(content: AnyElement, attachment: &Attachment, env: &BodyEnv, cx: &App) -> AnyElement {
	let context = ViewContext {
		guild_id: env.lookup.guild.map(Snowflake::get),
		channel_id: env.channel_id(),
		message_id: Some(env.message_id),
		user_id: None,
		attachment_id: Some(attachment.id.get()),
	};
	let actions = contextual_inline("attachment-action", context, INLINE, ATTACHMENT_ACTION_NODES, cx);
	if actions.is_empty() {
		return content;
	}
	div()
		.flex()
		.flex_col()
		.items_start()
		.child(content)
		.child(
			div()
				.flex()
				.items_center()
				.gap(env.theme.space_4)
				.mt(env.theme.space_2)
				.children(actions),
		)
		.into_any_element()
}

fn customized(
	content: AnyElement,
	attachment: &Attachment,
	over: &AttachmentOverride,
	look: Look,
	env: &BodyEnv,
	cx: &App,
) -> AnyElement {
	let theme = env.theme;
	let id = attachment.id.get();
	let mut shown = content;
	if over.blur {
		let group = SharedString::from(format!("attachment-blur-{id}"));
		shown = div()
			.id(("attachment-blur", id))
			.group(group.clone())
			.relative()
			.self_start()
			.child(shown)
			.child(blur_cover(attachment, look, group, env, cx))
			.into_any_element();
	}
	if let Some(tip) = &over.tooltip {
		shown = div()
			.id(("attachment-tip", id))
			.self_start()
			.tooltip(tooltip(tip.clone()))
			.child(shown)
			.into_any_element();
	}
	if let Some(alt) = &over.alt {
		shown = div()
			.flex()
			.flex_col()
			.items_start()
			.child(shown)
			.child(
				div()
					.max_w(rem(MAX_IMAGE_WIDTH))
					.text_size(theme.font_tiny)
					.text_color(theme.text_muted)
					.child(alt.clone()),
			)
			.into_any_element();
	}
	shown
}

fn blur_cover(
	attachment: &Attachment,
	look: Look,
	group: SharedString,
	env: &BodyEnv,
	cx: &App,
) -> AnyElement {
	let theme = env.theme;
	let backdrop = if is_image(attachment) {
		let (native_width, native_height) = dimensions(attachment.width, attachment.height);
		let (width, height) = sized(look, native_width, native_height);
		let (tiny_width, tiny_height) = ((width / BLUR_DIVISOR).max(1.), (height / BLUR_DIVISOR).max(1.));
		let source = image_source(attachment);
		div()
			.absolute()
			.size_full()
			.child(picture(
				&proxied_media(&source, false, tiny_width, tiny_height),
				attachment.placeholder.as_deref(),
				tiny_width,
				tiny_height,
				ObjectFit::Cover,
				theme,
				cx,
			))
			.into_any_element()
	} else {
		div()
			.absolute()
			.size_full()
			.bg(theme.background_tertiary)
			.into_any_element()
	};
	div()
		.absolute()
		.top(theme.space_4)
		.left_0()
		.right_0()
		.bottom_0()
		.flex()
		.items_center()
		.justify_center()
		.rounded(theme.radius_md)
		.overflow_hidden()
		.group_hover(group, |style| style.invisible())
		.child(backdrop)
		.child(
			div()
				.px(theme.space_12)
				.py(theme.space_4)
				.rounded_full()
				.bg(theme.background_floating)
				.text_size(theme.font_sm)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_primary)
				.child(cx.t("attachment-hover-reveal")),
		)
		.into_any_element()
}

fn image_source(attachment: &Attachment) -> String {
	attachment
		.url
		.clone()
		.or_else(|| attachment.proxy_url.clone())
		.unwrap_or_default()
}

fn attachment_source(attachment: &Attachment) -> String {
	attachment
		.proxy_url
		.clone()
		.or_else(|| attachment.url.clone())
		.unwrap_or_default()
}

fn attachment_file(attachment: &Attachment) -> FileInfo {
	FileInfo {
		name: attachment.filename.clone(),
		size: attachment
			.size
			.map(|bytes| format::file_size(bytes.max(0) as u64)),
		download: attachment.url.clone().unwrap_or_default(),
	}
}

fn video_attachment(attachment: &Attachment, look: Look, env: &BodyEnv, cx: &App) -> AnyElement {
	let (native_width, native_height) = dimensions(attachment.width, attachment.height);
	let (width, height) = sized(look, native_width, native_height);
	let url = attachment_source(attachment);
	let link = url.clone();
	let video = player::video(
		VideoSource {
			key: MediaKey {
				message: env.message_id,
				item: attachment.id.get(),
			},
			poster: video_poster(&url, (width * 2.) as u32, (height * 2.) as u32),
			url,
			placeholder: attachment.placeholder.as_deref(),
			width,
			height,
			file: Some(attachment_file(attachment)),
		},
		env,
		cx,
	);
	with_attachment_menu(&link, attachment.id.get(), video)
}

fn audio_attachment(attachment: &Attachment, env: &BodyEnv, cx: &App) -> AnyElement {
	let key = MediaKey {
		message: env.message_id,
		item: attachment.id.get(),
	};
	let url = attachment_source(attachment);
	let link = url.clone();
	if attachment.waveform.is_some() {
		let voice = player::voice_message(
			VoiceSource {
				key,
				url,
				waveform: attachment.waveform.as_deref(),
				duration: attachment.duration_secs,
			},
			env,
			cx,
		);
		return with_attachment_menu(&link, attachment.id.get(), voice);
	}
	let audio = player::audio(
		AudioSource {
			key,
			url,
			file: attachment_file(attachment),
		},
		env,
		cx,
	);
	with_attachment_menu(&link, attachment.id.get(), audio)
}

fn image_attachment(attachment: &Attachment, look: Look, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let (native_width, native_height) = dimensions(attachment.width, attachment.height);
	let (width, height) = sized(look, native_width, native_height);
	let source = image_source(attachment);
	let shown = proxied_media(&source, attachment_animated(attachment), width, height);
	let target = ImageTarget {
		url: source,
		name: attachment.filename.clone(),
		width: native_width as u32,
		height: native_height as u32,
	};
	let pointed = point_media(&target, &target.url, Some(attachment.id.get()));
	let dispatch = env.dispatch.clone();
	let at = env.message_at();
	let attachment_id = attachment.id.get();
	div()
		.id(("attachment", attachment_id))
		.on_mouse_down(MouseButton::Right, pointed)
		.mt(theme.space_4)
		.w(rem(width))
		.h(rem(height))
		.rounded(theme.radius_md)
		.overflow_hidden()
		.cursor_pointer()
		.on_click(move |event, window, cx| {
			if let Some(at) = at
				&& message_click_at(at, event, ClickTarget::Attachment(attachment_id), cx)
			{
				cx.stop_propagation();
				return;
			}
			dispatch(RowAction::OpenImage(target.clone()), window, cx)
		})
		.child(picture(
			&shown,
			attachment.placeholder.as_deref(),
			width,
			height,
			ObjectFit::Cover,
			theme,
			cx,
		))
		.into_any_element()
}

fn file_card(attachment: &Attachment, video: bool, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let url = attachment.url.clone().unwrap_or_default();
	let size = attachment
		.size
		.map(|bytes| format::file_size(bytes.max(0) as u64));
	let icon = if video { IconKind::Play } else { IconKind::File };
	let dispatch = env.dispatch.clone();
	let download_dispatch = env.dispatch.clone();
	let open_url = url.clone();
	div()
		.id(("file", attachment.id.get()))
		.mt(theme.space_4)
		.flex()
		.items_center()
		.gap(theme.space_12)
		.w(rem(432.))
		.max_w_full()
		.p(theme.space_12)
		.rounded(theme.radius_md)
		.border_1()
		.border_color(theme.code_border)
		.bg(theme.background_secondary)
		.child(Icon::new(icon).size(rem(32.)).color(theme.interactive_normal))
		.child(
			div()
				.flex()
				.flex_col()
				.flex_1()
				.min_w_0()
				.child(
					div()
						.id(("file-name", attachment.id.get()))
						.truncate()
						.cursor_pointer()
						.text_size(theme.font_md)
						.text_color(theme.link)
						.on_click(move |_, window, cx| {
							dispatch(RowAction::Download(open_url.clone()), window, cx)
						})
						.child(attachment.filename.clone()),
				)
				.children(size.map(|size| {
					div()
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.child(size)
				})),
		)
		.child(
			IconButton::new(("file-download", attachment.id.get()), IconKind::Download)
				.icon_size(rem(24.))
				.hit_size(rem(32.))
				.tooltip(cx.t("attachment-download"))
				.on_click(move |_, window, cx| {
					download_dispatch(RowAction::Download(url.clone()), window, cx);
				}),
		)
		.into_any_element()
}

pub fn stickers(items: &[StickerItem], env: &BodyEnv, cx: &App) -> Vec<AnyElement> {
	items
		.iter()
		.filter_map(|sticker| {
			let format = media::cdn::StickerFormat::from(i64::from(sticker.format_type?));
			let url = sticker_url(sticker.id.get(), format)?;
			let shown = div()
				.mt(env.theme.space_4)
				.size(rem(STICKER_SIZE))
				.child(picture(
					&url,
					None,
					STICKER_SIZE,
					STICKER_SIZE,
					ObjectFit::Contain,
					env.theme,
					cx,
				))
				.into_any_element();
			Some(plugin_menu(
				("sticker-menu", sticker.id.get() ^ env.message_id),
				shown,
				"sticker-context-menu",
				MenuTarget::sticker(env.lookup.guild, env.channel(), sticker.id),
				cx,
			))
		})
		.collect()
}

fn media_url(media: &EmbedMedia) -> Option<&str> {
	media.proxy_url.as_deref().or(media.url.as_deref())
}

fn direct_video(embed: &Embed) -> Option<&EmbedMedia> {
	embed.video.as_ref().filter(|video| video.proxy_url.is_some())
}

fn media_only(embed: &Embed) -> bool {
	embed.title.is_none()
		&& embed.description.is_none()
		&& embed.author.is_none()
		&& embed.fields.is_empty()
		&& embed.footer.is_none()
}

pub fn provider_player(embed: &Embed) -> Option<providers::Player> {
	let video = embed.video.as_ref();
	providers::player(&Candidate {
		url: embed.url.as_deref(),
		video_url: video.and_then(|video| video.url.as_deref()),
		video_size: video.and_then(|video| {
			Some((
				u32::try_from(video.width?).ok()?,
				u32::try_from(video.height?).ok()?,
			))
		}),
	})
}

pub fn hides_link(embed: &Embed) -> bool {
	match embed.kind {
		Some(EmbedKind::Image | EmbedKind::Gifv) => true,
		Some(EmbedKind::Video) => direct_video(embed).is_some() && media_only(embed),
		_ => false,
	}
}

pub fn embeds(items: &[Embed], env: &BodyEnv, previews: bool, cx: &App) -> Vec<AnyElement> {
	items
		.iter()
		.enumerate()
		.filter(|(_, embed)| {
			previews || matches!(embed.kind, None | Some(EmbedKind::Rich | EmbedKind::PollResult))
		})
		.filter_map(|(index, embed)| {
			let card = embed_card(index, embed, env, cx)?;
			Some(clickable_embed(index, card, env, cx))
		})
		.collect()
}

fn clickable_embed(index: usize, card: AnyElement, env: &BodyEnv, cx: &App) -> AnyElement {
	let Some(at) = env.message_at().filter(|_| watching_messages(cx)) else {
		return card;
	};
	div()
		.id(("embed-click", env.message_id ^ ((index as u64) << 56)))
		.on_click(move |event, _, cx| {
			if message_click_at(at, event, ClickTarget::Embed(index as u32), cx) {
				cx.stop_propagation();
			}
		})
		.child(card)
		.into_any_element()
}

fn bare_media(index: usize, embed: &Embed, env: &BodyEnv, cx: &App) -> Option<AnyElement> {
	let media = embed.image.as_ref().or(embed.thumbnail.as_ref())?;
	let (native_width, native_height) = dimensions(media.width, media.height);
	let (width, height) = fit(native_width, native_height, MAX_IMAGE_WIDTH, MAX_IMAGE_HEIGHT);
	let url = media_url(media)?.to_owned();
	let animated = embed_animated(media);
	let target = ImageTarget {
		url: media.url.clone().unwrap_or_else(|| url.clone()),
		name: String::new(),
		width: native_width as u32,
		height: native_height as u32,
	};
	let pointed = point_media(&target, &url, None);
	let dispatch = env.dispatch.clone();
	Some(
		div()
			.id(("embed-media", index))
			.on_mouse_down(MouseButton::Right, pointed)
			.mt(env.theme.space_4)
			.w(rem(width))
			.h(rem(height))
			.max_w_full()
			.rounded(env.theme.radius_md)
			.overflow_hidden()
			.cursor_pointer()
			.on_click(move |_, window, cx| dispatch(RowAction::OpenImage(target.clone()), window, cx))
			.child(picture(
				&proxied_media(&url, animated, width, height),
				media.placeholder.as_deref(),
				width,
				height,
				ObjectFit::Cover,
				env.theme,
				cx,
			))
			.into_any_element(),
	)
}

fn gifv(index: usize, embed: &Embed, env: &BodyEnv, cx: &App) -> Option<AnyElement> {
	let Some(video) = direct_video(embed) else {
		return bare_media(index, embed, env, cx);
	};
	let thumbnail = embed.thumbnail.as_ref();
	let sized = [Some(video), thumbnail]
		.into_iter()
		.flatten()
		.find(|media| media.width.is_some() && media.height.is_some())
		.unwrap_or(video);
	let (native_width, native_height) = dimensions(sized.width, sized.height);
	let (width, height) = fit(native_width, native_height, MAX_IMAGE_WIDTH, MAX_IMAGE_HEIGHT);
	let link = embed.url.clone().or_else(|| video.url.clone())?;
	let gif = player::gif_loop(
		LoopSource {
			key: embed_key(env, index),
			url: media_url(video)?.to_owned(),
			poster: thumbnail
				.and_then(media_url)
				.map(|url| proxied(url, width, height)),
			placeholder: thumbnail
				.and_then(|media| media.placeholder.as_deref())
				.or(video.placeholder.as_deref()),
			width,
			height,
		},
		env,
		cx,
	);
	Some(with_link_menu(&link, gif))
}

fn embedded_video(
	index: usize,
	embed: &Embed,
	video: &EmbedMedia,
	(max_width, max_height): (f32, f32),
	env: &BodyEnv,
	cx: &App,
) -> Option<AnyElement> {
	let thumbnail = embed.thumbnail.as_ref();
	let sized = if video.width.is_some() {
		video
	} else {
		thumbnail.unwrap_or(video)
	};
	let (native_width, native_height) = dimensions(sized.width, sized.height);
	let (width, height) = fit(native_width, native_height, max_width, max_height);
	let url = media_url(video)?.to_owned();
	let poster = thumbnail
		.and_then(media_url)
		.map(|url| proxied(url, width, height))
		.or_else(|| video_poster(&url, (width * 2.) as u32, (height * 2.) as u32));
	let link = embed
		.url
		.clone()
		.or_else(|| video.url.clone())
		.unwrap_or_else(|| url.clone());
	let player = player::video(
		VideoSource {
			key: embed_key(env, index),
			url,
			poster,
			placeholder: thumbnail
				.and_then(|media| media.placeholder.as_deref())
				.or(video.placeholder.as_deref()),
			width,
			height,
			file: None,
		},
		env,
		cx,
	);
	Some(with_link_menu(&link, player))
}

fn link_text(
	id: (&'static str, usize),
	text: String,
	url: Option<String>,
	env: &BodyEnv,
	weight: FontWeight,
) -> AnyElement {
	let theme = env.theme;
	let label = div().text_size(theme.font_md).font_weight(weight).child(text);
	match url {
		Some(url) => {
			let dispatch = env.dispatch.clone();
			label
				.id(id)
				.cursor_pointer()
				.text_color(theme.link)
				.hover(|style| style.underline())
				.on_click(move |_, window, cx| dispatch(RowAction::OpenLink(url.clone()), window, cx))
				.into_any_element()
		}
		None => label.text_color(theme.header_primary).into_any_element(),
	}
}

fn provider_line(embed: &Embed, key: usize, env: &BodyEnv) -> Option<AnyElement> {
	let theme = env.theme;
	let provider = embed.provider.as_ref()?;
	let name = provider.name.clone()?;
	let label = div()
		.text_size(theme.font_tiny)
		.text_color(theme.text_muted)
		.child(name);
	Some(match provider.url.clone() {
		Some(url) => {
			let dispatch = env.dispatch.clone();
			label
				.id(("embed-provider", key))
				.cursor_pointer()
				.hover(|style| style.underline())
				.on_click(move |_, window, cx| dispatch(RowAction::OpenLink(url.clone()), window, cx))
				.into_any_element()
		}
		None => label.into_any_element(),
	})
}

fn embed_card(index: usize, embed: &Embed, env: &BodyEnv, cx: &App) -> Option<AnyElement> {
	let theme = env.theme;
	match embed.kind {
		Some(EmbedKind::Gifv) => return gifv(index, embed, env, cx),
		Some(EmbedKind::Image) => return bare_media(index, embed, env, cx),
		Some(EmbedKind::Components) => return None,
		_ => {}
	}
	if let Some(video) = direct_video(embed)
		&& media_only(embed)
	{
		return embedded_video(index, embed, video, (MAX_IMAGE_WIDTH, MAX_IMAGE_HEIGHT), env, cx);
	}
	let provider_player = provider_player(embed);
	let video_player = provider_player
		.as_ref()
		.filter(|player| player.layout == Layout::Video);
	let widget_player = provider_player
		.as_ref()
		.filter(|player| player.layout == Layout::Widget);
	let accent = embed
		.color
		.and_then(|color| u32::try_from(color).ok())
		.map_or(theme.interactive_muted, role_color);
	let key = index * 16;
	let mut column = div().flex().flex_col().flex_1().min_w_0().gap(theme.space_4);
	column = column.children(provider_line(embed, key, env));
	if let Some(author) = &embed.author {
		let icon = author
			.proxy_icon_url
			.as_deref()
			.or(author.icon_url.as_deref())
			.map(|url| picture(url, None, 24., 24., ObjectFit::Cover, theme, cx));
		column = column.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.children(icon.map(|icon| {
					div()
						.flex_none()
						.size(rem(24.))
						.rounded_full()
						.overflow_hidden()
						.child(icon)
				}))
				.child(link_text(
					("embed-author", key),
					author.name.clone(),
					author.url.clone(),
					env,
					FontWeight::SEMIBOLD,
				)),
		);
	}
	if let Some(title) = &embed.title {
		column = column.child(link_text(
			("embed-title", key + 1),
			title.clone(),
			embed.url.clone(),
			env,
			FontWeight::SEMIBOLD,
		));
	}
	if let Some(description) = embed.description.as_ref().filter(|_| video_player.is_none()) {
		let body = BodyEnv::new(
			env.theme,
			env.lookup,
			env.dispatch,
			env.message_id,
			env.revealed,
			env.prefs,
		)
		.text_size(theme.font_sm);
		column = column.child(render_body(&parse(description), &body, cx));
	}
	if !embed.fields.is_empty() {
		column = column.child(fields(embed, env, cx));
	}
	if let Some(video) = direct_video(embed) {
		column = column.children(embedded_video(
			index,
			embed,
			video,
			(EMBED_IMAGE_WIDTH, EMBED_IMAGE_HEIGHT),
			env,
			cx,
		));
	} else if let Some(player) = video_player {
		let frame = provider::video(embed_key(env, index), player, embed, env, cx);
		column = column.child(match embed.url.as_deref() {
			Some(link) => with_link_menu(link, frame),
			None => frame,
		});
	} else {
		let large = embed.image.as_ref().or_else(|| {
			embed
				.video
				.as_ref()
				.and(embed.thumbnail.as_ref().or(embed.video.as_ref()))
		});
		if let Some(media) = large
			&& let Some(url) = media_url(media)
		{
			column = column.child(large_media(index, media, url, embed, env, cx));
		}
	}
	if let Some(player) = widget_player {
		let frame = provider::widget(embed_key(env, index), player, embed, env, cx);
		column = column.child(match embed.url.as_deref() {
			Some(link) => with_link_menu(link, frame),
			None => frame,
		});
	}
	if embed.footer.is_some() || embed.timestamp.is_some() {
		let footer = embed.footer.as_ref().map(|footer| footer.text.clone());
		let stamp = embed
			.timestamp
			.map(|time| format::message_time(time, cx).to_string());
		let text = [footer, stamp]
			.into_iter()
			.flatten()
			.collect::<Vec<_>>()
			.join(" \u{2022} ");
		column = column.child(
			div()
				.text_size(theme.font_tiny)
				.text_color(theme.text_muted)
				.child(text),
		);
	}
	let thumbnail = embed
		.thumbnail
		.as_ref()
		.filter(|_| embed.image.is_none() && embed.video.is_none() && widget_player.is_none())
		.and_then(|media| {
			Some(picture(
				&proxied_media(media_url(media)?, embed_animated(media), THUMBNAIL, THUMBNAIL),
				media.placeholder.as_deref(),
				THUMBNAIL,
				THUMBNAIL,
				ObjectFit::Cover,
				theme,
				cx,
			))
		});
	Some(
		div()
			.mt(theme.space_4)
			.flex()
			.w(rem(EMBED_WIDTH))
			.max_w_full()
			.rounded_r(theme.radius_sm)
			.border_l_4()
			.border_color(accent)
			.bg(theme.background_secondary)
			.child(
				div()
					.flex()
					.flex_1()
					.min_w_0()
					.gap(theme.space_16)
					.py(theme.space_8)
					.pl(theme.space_12)
					.pr(theme.space_16)
					.child(column)
					.children(thumbnail.map(|thumbnail| {
						div()
							.flex_none()
							.size(rem(THUMBNAIL))
							.rounded(theme.radius_sm)
							.overflow_hidden()
							.child(thumbnail)
					})),
			)
			.into_any_element(),
	)
}

fn fields(embed: &Embed, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	div()
		.flex()
		.flex_wrap()
		.gap(theme.space_8)
		.children(embed.fields.iter().map(|field| {
			let value = BodyEnv::new(
				env.theme,
				env.lookup,
				env.dispatch,
				env.message_id,
				env.revealed,
				env.prefs,
			)
			.text_size(theme.font_sm);
			div()
				.flex()
				.flex_col()
				.min_w_0()
				.when(field.inline, |cell| cell.w(rem(FIELD_WIDTH)))
				.when(!field.inline, |cell| cell.w_full())
				.child(
					div()
						.text_size(theme.font_sm)
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(theme.header_primary)
						.child(field.name.clone()),
				)
				.child(render_body(&parse(&field.value), &value, cx))
		}))
		.into_any_element()
}

fn large_media(
	index: usize,
	media: &EmbedMedia,
	url: &str,
	embed: &Embed,
	env: &BodyEnv,
	cx: &App,
) -> AnyElement {
	let theme = env.theme;
	let (native_width, native_height) = dimensions(media.width, media.height);
	let (width, height) = fit(native_width, native_height, EMBED_IMAGE_WIDTH, EMBED_IMAGE_HEIGHT);
	let playable = embed.video.is_some();
	let target_url = embed
		.url
		.clone()
		.or_else(|| embed.video.as_ref().and_then(|v| v.url.clone()));
	let image_target = ImageTarget {
		url: media.url.clone().unwrap_or_else(|| url.to_owned()),
		name: String::new(),
		width: native_width as u32,
		height: native_height as u32,
	};
	let pointed = point_media(&image_target, url, None);
	let link_target = target_url.clone().map(|url| point_link(&url));
	let dispatch = env.dispatch.clone();
	div()
		.id(("embed-large", index))
		.map(|frame| match (playable, link_target) {
			(true, Some(link)) => frame.on_mouse_down(MouseButton::Right, link),
			(true, None) => frame,
			(false, _) => frame.on_mouse_down(MouseButton::Right, pointed),
		})
		.relative()
		.mt(theme.space_4)
		.w(rem(width))
		.h(rem(height))
		.max_w_full()
		.rounded(theme.radius_sm)
		.overflow_hidden()
		.cursor_pointer()
		.on_click(move |_, window, cx| {
			let action = match (&target_url, playable) {
				(Some(url), true) => RowAction::OpenLink(url.clone()),
				_ => RowAction::OpenImage(image_target.clone()),
			};
			dispatch(action, window, cx);
		})
		.child(picture(
			&proxied_media(url, embed_animated(media), width, height),
			media.placeholder.as_deref(),
			width,
			height,
			ObjectFit::Cover,
			theme,
			cx,
		))
		.when(playable, |frame| {
			frame.child(
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
							.size(rem(48.))
							.rounded_full()
							.bg(theme.overlay)
							.child(
								Icon::new(IconKind::Play)
									.size(rem(24.))
									.color(theme.text_on_brand),
							),
					),
			)
		})
		.into_any_element()
}

pub fn emoji_key(emoji: &PartialEmoji) -> String {
	match (&emoji.id, &emoji.name) {
		(Some(id), name) => format!("{}:{id}", name.as_deref().unwrap_or("emoji")),
		(None, name) => name.clone().unwrap_or_default(),
	}
}

fn reaction_chip(
	index: usize,
	message: &Message,
	reaction: &Reaction,
	env: &BodyEnv,
	cx: &App,
) -> AnyElement {
	let theme = env.theme;
	let glyph = match reaction.emoji.id {
		Some(id) => {
			let pixels = (REACTION_EMOJI * 2.) as u32;
			let url = media::cdn::emoji_url(id.get(), reaction.emoji.animated, pixels);
			let image = cx.global::<Images>().get(&url, SizeHint::square(pixels));
			div()
				.flex_none()
				.size(rem(REACTION_EMOJI))
				.children(image.map(|image| img(image).id(frame_id(&url)).size_full()))
				.into_any_element()
		}
		None => unicode_emoji(
			reaction.emoji.name.as_deref().unwrap_or_default(),
			rem(REACTION_EMOJI),
			cx,
		),
	};
	let key = emoji_key(&reaction.emoji);
	let add = !reaction.me;
	let message_id = message.id;
	let channel_id = message.channel_id;
	let dispatch = env.dispatch.clone();
	let me = reaction.me;
	let tip_key = key.clone();
	let tip_label: SharedString = reaction
		.emoji
		.name
		.clone()
		.map(|name| {
			if reaction.emoji.id.is_some() {
				format!(":{name}:")
			} else {
				name
			}
		})
		.unwrap_or_default()
		.into();
	let (guild_id, count) = (message.guild_id, reaction.count);
	let chip = div()
		.id(SharedString::from(format!("reaction-{}-{index}", message.id)))
		.flex()
		.items_center()
		.gap(rem(6.))
		.h(rem(26.))
		.px(theme.space_8)
		.rounded(theme.radius_md)
		.border_1()
		.cursor_pointer()
		.text_size(theme.font_sm)
		.font_weight(FontWeight::SEMIBOLD)
		.when(me, |chip| {
			chip.border_color(theme.brand)
				.bg(with_alpha(theme.brand, 0.24))
				.text_color(theme.mention_text)
		})
		.when(!me, |chip| {
			chip.border_color(theme.button_secondary)
				.bg(theme.button_secondary)
				.text_color(theme.interactive_normal)
				.hover(|style| style.border_color(theme.interactive_muted))
		})
		.on_click(move |_, window, cx| {
			dispatch(
				RowAction::React {
					channel_id,
					message_id,
					emoji: key.clone(),
					add,
				},
				window,
				cx,
			);
		})
		.tooltip(move |_, cx| {
			reaction_tip::build(
				channel_id,
				message_id,
				guild_id,
				tip_key.clone(),
				tip_label.clone(),
				count,
				cx,
			)
		})
		.child(glyph)
		.child(reaction.count.to_string())
		.into_any_element();
	let emoji = EmojiRef {
		id: reaction.emoji.id.map(Snowflake::get),
		name: reaction.emoji.name.clone().unwrap_or_default(),
		animated: reaction.emoji.animated,
	};
	plugin_menu(
		SharedString::from(format!("reaction-menu-{}-{index}", message.id)),
		chip,
		"reaction-context-menu",
		MenuTarget::message(message, env.lookup.guild, env.lookup.state.current_user_id()).reaction(emoji),
		cx,
	)
}

pub fn reactions(message: &Message, env: &BodyEnv, cx: &App) -> Option<AnyElement> {
	if message.reactions.is_empty() {
		return None;
	}
	Some(
		div()
			.flex()
			.flex_wrap()
			.gap(rem(4.))
			.mt(env.theme.space_4)
			.children(
				message
					.reactions
					.iter()
					.enumerate()
					.map(|(index, reaction)| reaction_chip(index, message, reaction, env, cx)),
			)
			.into_any_element(),
	)
}

pub struct PollView<'a> {
	pub message: &'a Message,
	pub picks: Option<&'a Vec<i64>>,
	pub mine: bool,
}

fn poll_finished(poll: &Poll) -> bool {
	poll.results.as_ref().is_some_and(|results| results.is_finalized)
		|| poll.expiry.is_some_and(|expiry| expiry <= Utc::now())
}

pub fn poll(view: &PollView, env: &BodyEnv, cx: &App) -> AnyElement {
	let theme = env.theme;
	let message = view.message;
	let Some(poll) = &message.poll else {
		return div().into_any_element();
	};
	let (channel_id, message_id) = (message.channel_id, message.id);
	let counts = poll.results.as_ref();
	let count_of = |id: i64| {
		counts
			.and_then(|results| results.answer_counts.iter().find(|entry| entry.id == id))
			.map_or((0, false), |entry| (entry.count, entry.me_voted))
	};
	let finished = poll_finished(poll);
	let multi = poll.allow_multiselect;
	let voted: Vec<i64> = poll
		.answers
		.iter()
		.filter_map(|answer| answer.answer_id)
		.filter(|id| count_of(*id).1)
		.collect();
	let current: Vec<i64> = view.picks.cloned().unwrap_or_else(|| voted.clone());
	let total: i64 = poll
		.answers
		.iter()
		.map(|answer| count_of(answer.answer_id.unwrap_or_default()).0)
		.sum();
	let rows = poll.answers.iter().enumerate().map(|(index, answer)| {
		let answer_id = answer.answer_id.unwrap_or_default();
		let (count, _) = count_of(answer_id);
		let chosen = current.contains(&answer_id);
		let share = if total > 0 {
			count as f32 / total as f32
		} else {
			0.
		};
		let fill = if chosen {
			theme.brand
		} else {
			theme.interactive_muted
		};
		let dispatch = env.dispatch.clone();
		let current = current.clone();
		div()
			.id(SharedString::from(format!("poll-answer-{message_id}-{index}")))
			.relative()
			.flex()
			.items_center()
			.justify_between()
			.gap(theme.space_8)
			.h(rem(40.))
			.px(theme.space_12)
			.rounded(theme.radius_md)
			.overflow_hidden()
			.bg(theme.background_tertiary)
			.when(!finished, |row| {
				row.cursor_pointer()
					.hover(|style| style.bg(theme.modifier_hover))
					.on_click(move |_, window, cx| {
						let action = if multi {
							RowAction::PollToggle {
								message_id,
								answer_id,
								current: current.clone(),
							}
						} else {
							RowAction::PollVote {
								channel_id,
								message_id,
								answer_ids: if chosen { Vec::new() } else { vec![answer_id] },
							}
						};
						dispatch(action, window, cx)
					})
			})
			.child(
				div()
					.absolute()
					.left_0()
					.top_0()
					.bottom_0()
					.w(gpui::relative(share))
					.bg(with_alpha(fill, 0.45)),
			)
			.child(
				div()
					.relative()
					.flex()
					.flex_none()
					.items_center()
					.justify_center()
					.size(rem(16.))
					.rounded_full()
					.border_1()
					.border_color(if chosen {
						theme.brand
					} else {
						theme.interactive_muted
					})
					.when(chosen, |dot| {
						dot.bg(theme.brand).child(
							Icon::new(IconKind::Check)
								.size(rem(12.))
								.color(theme.text_on_brand),
						)
					}),
			)
			.child(
				div()
					.relative()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_size(theme.font_sm)
					.font_weight(FontWeight::MEDIUM)
					.text_color(theme.header_primary)
					.child(answer.poll_media.text.clone().unwrap_or_default()),
			)
			.child(
				div()
					.relative()
					.flex_none()
					.text_size(theme.font_sm)
					.text_color(theme.header_secondary)
					.child(format!("{}%", (share * 100.).round() as u32)),
			)
	});
	let votes: SharedString = cx.t_args("poll-votes", &[("count", ArgValue::from(total))]);
	let submit_dispatch = env.dispatch.clone();
	let end_dispatch = env.dispatch.clone();
	let submit_ids = current.clone();
	let dirty = view.picks.is_some_and(|picks| *picks != voted);
	div()
		.mt(theme.space_4)
		.flex()
		.flex_col()
		.gap(theme.space_8)
		.w(rem(432.))
		.max_w_full()
		.p(theme.space_12)
		.rounded(theme.radius_md)
		.border_1()
		.border_color(theme.code_border)
		.bg(theme.background_secondary)
		.child(
			div()
				.text_size(theme.font_md)
				.font_weight(FontWeight::BOLD)
				.text_color(theme.header_primary)
				.child(poll.question.text.clone().unwrap_or_default()),
		)
		.child(
			div()
				.text_size(theme.font_tiny)
				.text_color(theme.text_muted)
				.child(cx.t(if finished {
					"poll-closed"
				} else if multi {
					"poll-select-multiple"
				} else {
					"poll-select-one"
				})),
		)
		.children(rows)
		.child(
			div()
				.flex()
				.items_center()
				.gap(theme.space_8)
				.child(
					div()
						.flex_1()
						.text_size(theme.font_tiny)
						.text_color(theme.text_muted)
						.child(votes),
				)
				.when(multi && !finished && dirty, |footer| {
					footer.child(
						Button::new(SharedString::from(format!("poll-vote-{message_id}")))
							.small()
							.primary()
							.label(cx.t("poll-vote"))
							.on_click(move |_, window, cx| {
								submit_dispatch(
									RowAction::PollVote {
										channel_id,
										message_id,
										answer_ids: submit_ids.clone(),
									},
									window,
									cx,
								)
							}),
					)
				})
				.when(view.mine && !finished, |footer| {
					footer.child(
						Button::new(SharedString::from(format!("poll-end-{message_id}")))
							.small()
							.outline()
							.label(cx.t("poll-end"))
							.on_click(move |_, window, cx| {
								end_dispatch(
									RowAction::PollEnd {
										channel_id,
										message_id,
									},
									window,
									cx,
								)
							}),
					)
				}),
		)
		.into_any_element()
}
