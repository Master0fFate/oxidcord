use gpui::{
	AnyElement, App, FontWeight, Hsla, InteractiveElement as _, IntoElement as _, ObjectFit,
	ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, StyledImage as _, div,
	img, prelude::FluentBuilder as _,
};
use media::SizeHint;
use protocol::Snowflake;
use session::Command;
use state::{BadgeColor, PluginBadge, State, UserOverride};

use super::bridge::PluginBridge;
use super::icon::PluginIcon;
use super::view::badge_element;
use crate::components::tooltip;
use crate::images::{Images, frame_id};
use crate::lookup::avatar_for;
use crate::theme::{Theme, rem};

const BADGE_SIZE: f32 = 16.;
const BADGE_PIXELS: u32 = 32;

pub fn display_name(state: &State, guild: Option<Snowflake>, user: Snowflake) -> String {
	state
		.user_override(guild, user)
		.and_then(|over| over.name.clone())
		.unwrap_or_else(|| state.display_name(guild, user))
}

pub fn avatar_url(state: &State, guild: Option<Snowflake>, user: Snowflake, size: u32) -> String {
	state
		.user_override(guild, user)
		.and_then(|over| over.avatar.clone())
		.unwrap_or_else(|| avatar_for(state, user, size))
}

pub fn decoration_of(over: Option<&UserOverride>) -> Option<SharedString> {
	over.and_then(|over| over.decoration.clone()).map(Into::into)
}

fn badge_color(color: &BadgeColor, theme: &Theme) -> Option<Hsla> {
	match color {
		BadgeColor::Token(token) => theme.token(token),
		BadgeColor::Rgba(rgba) => Some(gpui::rgba(*rgba).into()),
	}
}

fn badge_face(badge: &PluginBadge, color: Hsla, theme: &Theme, cx: &App) -> AnyElement {
	if let Some(url) = &badge.image {
		let image = cx.global::<Images>().get(url, SizeHint::square(BADGE_PIXELS));
		return div()
			.size(rem(BADGE_SIZE))
			.children(image.map(|image| {
				img(image)
					.id(frame_id(url))
					.size_full()
					.object_fit(ObjectFit::Contain)
			}))
			.into_any_element();
	}
	if let Some(icon) = &badge.icon {
		return PluginIcon::new(icon.clone())
			.size(rem(BADGE_SIZE))
			.color(color)
			.into_any_element();
	}
	match &badge.label {
		Some(label) => badge_element(label, color, theme),
		None => div().into_any_element(),
	}
}

fn badge(
	index: usize,
	user: Snowflake,
	plugin: Option<&str>,
	badge: &PluginBadge,
	theme: &Theme,
	cx: &App,
) -> AnyElement {
	let color = badge
		.color
		.as_ref()
		.and_then(|color| badge_color(color, theme))
		.unwrap_or(theme.interactive_normal);
	let clickable = plugin
		.zip(badge.key.as_deref())
		.map(|(plugin, key)| (plugin.to_owned(), key.to_owned()));
	div()
		.id(SharedString::from(format!("plugin-user-badge:{user}:{index}")))
		.flex()
		.flex_none()
		.items_center()
		.when_some(clickable, |element, (plugin_id, key)| {
			element.cursor_pointer().on_click(move |_, _, cx| {
				cx.stop_propagation();
				cx.global::<PluginBridge>().send(
					Command::PluginBadgeClick {
						plugin_id: plugin_id.clone(),
						user_id: user,
						key: key.clone(),
					},
					cx,
				);
			})
		})
		.when(!badge.tooltip.is_empty(), |element| {
			element.tooltip(tooltip(badge.tooltip.clone()))
		})
		.child(badge_face(badge, color, theme, cx))
		.into_any_element()
}

fn muted(text: &str, theme: &Theme) -> AnyElement {
	div()
		.flex_none()
		.text_size(theme.font_xs)
		.font_weight(FontWeight::NORMAL)
		.text_color(theme.text_muted)
		.child(text.to_owned())
		.into_any_element()
}

pub fn tags(over: &UserOverride, user: Snowflake, theme: &Theme, cx: &App) -> Vec<AnyElement> {
	let mut out = Vec::with_capacity(2 + over.badges.len());
	if let Some(suffix) = &over.suffix {
		out.push(muted(suffix, theme));
	}
	if let Some(pronouns) = &over.pronouns {
		out.push(muted(pronouns, theme));
	}
	out.extend(
		over.badges
			.iter()
			.enumerate()
			.map(|(index, entry)| badge(index, user, over.plugin.as_deref(), entry, theme, cx)),
	);
	out
}

pub fn badges(over: &UserOverride, user: Snowflake, theme: &Theme, cx: &App) -> Vec<AnyElement> {
	over.badges
		.iter()
		.enumerate()
		.map(|(index, entry)| badge(index, user, over.plugin.as_deref(), entry, theme, cx))
		.collect()
}

pub fn channel_badges(
	channel: Snowflake,
	badges: &[PluginBadge],
	theme: &Theme,
	cx: &App,
) -> Vec<AnyElement> {
	badges
		.iter()
		.enumerate()
		.map(|(index, entry)| badge(index, channel, None, entry, theme, cx))
		.collect()
}
