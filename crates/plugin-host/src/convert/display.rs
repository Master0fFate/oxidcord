use super::view::{ImagePolicy, clip_line, sanitize_color, valid_icon, valid_key};
use crate::api::model::{Badge, DisplayOverrideFull};

const MAX_BADGES: usize = 8;
const MAX_TOOLTIP: usize = 128;
const MAX_LABEL: usize = 32;
const MAX_SUFFIX: usize = 64;
const MAX_PRONOUNS: usize = 32;
const MAX_NAME_CHARS: usize = 80;

fn text(value: Option<String>, max_bytes: usize) -> Option<String> {
	value
		.map(|value| clip_line(&value, max_bytes))
		.filter(|value| !value.trim().is_empty())
}

fn image(value: Option<String>, images: &ImagePolicy) -> Option<String> {
	value.filter(|url| images.allows(url))
}

pub fn sanitize_badges(badges: Vec<Badge>, images: &ImagePolicy) -> Vec<Badge> {
	badges
		.into_iter()
		.map(|badge| Badge {
			icon: badge.icon.filter(|icon| valid_icon(icon)),
			image: image(badge.image, images),
			label: text(badge.label, MAX_LABEL),
			tooltip: clip_line(&badge.tooltip, MAX_TOOLTIP),
			color: badge.color.as_ref().and_then(sanitize_color),
			key: badge.key.filter(|key| valid_key(key)),
		})
		.filter(|badge| badge.icon.is_some() || badge.image.is_some() || badge.label.is_some())
		.take(MAX_BADGES)
		.collect()
}

pub fn sanitize_display_full(over: DisplayOverrideFull, images: &ImagePolicy) -> Option<DisplayOverrideFull> {
	let name = over
		.name
		.map(|name| {
			name.chars()
				.filter(|ch| !ch.is_control())
				.take(MAX_NAME_CHARS)
				.collect()
		})
		.filter(|name: &String| !name.trim().is_empty());
	let sanitized = DisplayOverrideFull {
		name,
		suffix: text(over.suffix, MAX_SUFFIX),
		color: over.color.as_ref().and_then(sanitize_color),
		badges: sanitize_badges(over.badges, images),
		avatar: image(over.avatar, images),
		banner: image(over.banner, images),
		decoration: image(over.decoration, images),
		pronouns: text(over.pronouns, MAX_PRONOUNS),
	};
	let empty = sanitized.name.is_none()
		&& sanitized.suffix.is_none()
		&& sanitized.color.is_none()
		&& sanitized.badges.is_empty()
		&& sanitized.avatar.is_none()
		&& sanitized.banner.is_none()
		&& sanitized.decoration.is_none()
		&& sanitized.pronouns.is_none();
	(!empty).then_some(sanitized)
}
