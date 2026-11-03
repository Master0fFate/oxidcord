use gpui::{
	AnyElement, App, Background, Div, HighlightStyle, Hsla, IntoElement as _, ObjectFit, ParentElement as _,
	Rgba, SharedString, Styled as _, StyledImage as _, StyledText, div, img, linear_color_stop,
	linear_gradient, prelude::FluentBuilder as _,
};
use media::SizeHint;
use protocol::{DisplayNameEffect, NameplatePalette, Snowflake};
use state::State;

use crate::components::TagChip;
use crate::images::Images;
use crate::theme::{Theme, with_alpha};

const DECORATION_PIXELS: u32 = 256;
const TAG_BADGE_PIXELS: u32 = 32;
const MAX_TAG_CHARS: usize = 4;
const MAX_STYLE_COLORS: usize = 5;
const PLATE_PIXELS: SizeHint = SizeHint::new(320, 64);
const PLATE_TINT_ALPHA: f32 = 0.22;
const PROFILE_TINT: f32 = 0.3;
const PROFILE_GRADIENT_ANGLE: f32 = 180.;

#[derive(Debug, Clone, PartialEq)]
pub struct GuildTag {
	pub text: SharedString,
	pub badge: Option<SharedString>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Nameplate {
	pub url: SharedString,
	pub tint: Option<Hsla>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NameStyle {
	effect: DisplayNameEffect,
	colors: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Flair {
	pub decoration: Option<SharedString>,
	pub tag: Option<GuildTag>,
	pub nameplate: Option<Nameplate>,
	pub name_style: Option<NameStyle>,
}

fn palette_tint(palette: &NameplatePalette) -> Option<Hsla> {
	let rgb = match palette {
		NameplatePalette::Crimson => 0xc4283c,
		NameplatePalette::Berry => 0x9c2f8f,
		NameplatePalette::Sky => 0x3c9bdc,
		NameplatePalette::Teal => 0x1fa89b,
		NameplatePalette::Forest => 0x2e8b4a,
		NameplatePalette::BubbleGum => 0xe96bae,
		NameplatePalette::Violet => 0x7a55d9,
		NameplatePalette::Cobalt => 0x3453d1,
		NameplatePalette::Clover => 0x5bae3b,
		NameplatePalette::Lemon => 0xd7c233,
		NameplatePalette::White => 0xe8e8e8,
		NameplatePalette::None | NameplatePalette::Unknown(_) => return None,
	};
	Some(gpui::rgb(rgb).into())
}

impl Flair {
	pub fn tag_chip(&self) -> Option<TagChip> {
		self.tag
			.as_ref()
			.map(|tag| TagChip::new(tag.text.clone()).badge(tag.badge.clone()))
	}
}

pub fn flair(state: &State, guild: Option<Snowflake>, user: Snowflake) -> Flair {
	let found = state.cosmetics(guild, user);
	let decoration = state
		.user_override(guild, user)
		.and_then(|over| over.decoration.clone())
		.or_else(|| {
			let asset = found.decoration?.asset.as_deref()?;
			media::cdn::avatar_decoration_url(asset, DECORATION_PIXELS)
		})
		.map(Into::into);
	let tag = found.tag.and_then(|tag| {
		let badge = tag
			.badge
			.as_deref()
			.zip(tag.identity_guild_id)
			.and_then(|(badge, guild)| media::cdn::guild_tag_badge_url(guild.get(), badge, TAG_BADGE_PIXELS));
		Some(GuildTag {
			text: tag
				.tag
				.as_deref()?
				.chars()
				.take(MAX_TAG_CHARS)
				.collect::<String>()
				.into(),
			badge: badge.map(Into::into),
		})
	});
	let nameplate = found.nameplate.and_then(|plate| {
		Some(Nameplate {
			url: media::cdn::nameplate_url(&plate.asset)?.into(),
			tint: palette_tint(&plate.palette),
		})
	});
	let name_style = found.name_style.map(|style| NameStyle {
		effect: style.effect_id,
		colors: style.colors.iter().copied().take(MAX_STYLE_COLORS).collect(),
	});
	Flair {
		decoration,
		tag,
		nameplate,
		name_style,
	}
}

fn rgb_of(color: u32) -> Rgba {
	gpui::rgb(color & 0x00ff_ffff)
}

fn mix(from: Rgba, to: Rgba, amount: f32) -> Hsla {
	Rgba {
		r: from.r + (to.r - from.r) * amount,
		g: from.g + (to.g - from.g) * amount,
		b: from.b + (to.b - from.b) * amount,
		a: 1.,
	}
	.into()
}

impl NameStyle {
	fn gradient(&self) -> bool {
		self.colors.len() > 1
			&& matches!(
				self.effect,
				DisplayNameEffect::Gradient
					| DisplayNameEffect::Glow
					| DisplayNameEffect::Prism
					| DisplayNameEffect::Gummy
			)
	}

	fn color_at(&self, position: f32) -> Hsla {
		let segments = self.colors.len() - 1;
		let scaled = position * segments as f32;
		let index = (scaled as usize).min(segments - 1);
		mix(
			rgb_of(self.colors[index]),
			rgb_of(self.colors[index + 1]),
			scaled - index as f32,
		)
	}
}

pub fn name_text(name: SharedString, style: Option<&NameStyle>) -> AnyElement {
	let Some(style) = style else {
		return name.into_any_element();
	};
	let runs: Vec<_> = if style.gradient() {
		let last = name.chars().count().saturating_sub(1).max(1) as f32;
		name.char_indices()
			.enumerate()
			.map(|(index, (start, glyph))| {
				let color = style.color_at(index as f32 / last);
				(start..start + glyph.len_utf8(), HighlightStyle::color(color))
			})
			.collect()
	} else {
		let color = rgb_of(style.colors[0]).into();
		vec![(0..name.len(), HighlightStyle::color(color))]
	};
	StyledText::new(name).with_highlights(runs).into_any_element()
}

pub fn nameplate_backdrop(plate: &Nameplate, theme: &Theme, cx: &App) -> Div {
	let image = cx.global::<Images>().get_still(&plate.url, PLATE_PIXELS);
	div()
		.absolute()
		.top_0()
		.left_0()
		.size_full()
		.rounded(theme.radius_md)
		.overflow_hidden()
		.when_some(plate.tint, |backdrop, tint| {
			backdrop.bg(with_alpha(tint, PLATE_TINT_ALPHA))
		})
		.children(image.map(|image| img(image).size_full().object_fit(ObjectFit::Cover)))
}

pub fn profile_gradient(colors: [u32; 2], base: Hsla) -> Background {
	let base = base.to_rgb();
	let stop = |color, position| linear_color_stop(mix(base, rgb_of(color), PROFILE_TINT), position);
	linear_gradient(PROFILE_GRADIENT_ANGLE, stop(colors[0], 0.), stop(colors[1], 1.))
}
