use std::collections::BTreeSet;

use url::Url;

use crate::api::model::{
	Align, Axis, BadgeNode, ButtonNode, ButtonVariant, CheckNode, Color, EmbedCard, EmojiNode, EmojiRef,
	IconNode, ImageNode, ImageSource, InputNode, Justify, Length, LinkNode, NodeKind, Radius, SectionNode,
	SelectNode, SelectOption, SliderNode, Space, StackNode, Style, TabsNode, TextNode, TextSize,
	TimestampNode, Tone, ViewNode, ViewTree, Weight,
};
use crate::api::{UiBox, UiNode, UiText, UiTree};

const MAX_VIEW_NODES: usize = 2_000;
const MAX_VIEW_DEPTH: usize = 32;
const MAX_CHILDREN: usize = 500;
const MAX_TEXT: usize = 4_096;
const MAX_MARKDOWN: usize = 8_000;
const MAX_LABEL: usize = 128;
const MAX_TOOLTIP: usize = 256;
const MAX_KEY: usize = 128;
const MAX_URL: usize = 2_048;
const MAX_OPTIONS: usize = 50;
const MAX_TABS: usize = 16;
const MAX_IMAGE_SIZE: u32 = 512;
const MAX_PX: u32 = 4_096;
const DISCORD_IMAGE_HOSTS: [&str; 2] = ["cdn.discordapp.com", "media.discordapp.net"];

pub const COLOR_TOKENS: [&str; 24] = [
	"background-primary",
	"background-secondary",
	"background-tertiary",
	"background-floating",
	"modifier-hover",
	"modifier-selected",
	"modifier-active",
	"text-normal",
	"text-muted",
	"header-primary",
	"header-secondary",
	"interactive-normal",
	"interactive-hover",
	"interactive-muted",
	"brand",
	"text-on-brand",
	"link",
	"danger",
	"success",
	"warning",
	"mention-background",
	"mention-text",
	"divider",
	"input-background",
];

#[derive(Debug, Clone, Default)]
pub struct ImagePolicy {
	any_https: bool,
	hosts: BTreeSet<String>,
}

impl ImagePolicy {
	pub fn new(any_https: bool, hosts: BTreeSet<String>) -> Self {
		Self { any_https, hosts }
	}

	pub fn allows(&self, url: &str) -> bool {
		if url.len() > MAX_URL {
			return false;
		}
		let Ok(parsed) = Url::parse(url) else {
			return false;
		};
		if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
			return false;
		}
		let Some(host) = parsed.host_str() else {
			return false;
		};
		DISCORD_IMAGE_HOSTS.contains(&host) || self.any_https || self.hosts.contains(host)
	}
}

pub fn clip(text: &str, max_bytes: usize) -> String {
	if text.len() <= max_bytes {
		return text.to_owned();
	}
	let mut end = max_bytes;
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	text[..end].to_owned()
}

pub fn clip_line(text: &str, max_bytes: usize) -> String {
	clip(text, max_bytes)
		.chars()
		.filter(|character| !character.is_control())
		.collect()
}

pub fn valid_icon(name: &str) -> bool {
	!name.is_empty()
		&& name.len() <= 48
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn valid_key(key: &str) -> bool {
	!key.is_empty() && key.len() <= MAX_KEY && key.bytes().all(|byte| byte.is_ascii_graphic())
}

pub fn web_url(url: &str) -> bool {
	url.len() <= MAX_URL
		&& Url::parse(url)
			.is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.has_host())
}

pub fn sanitize_color(color: &Color) -> Option<Color> {
	match color {
		Color::Token(token) => COLOR_TOKENS.contains(&token.as_str()).then(|| color.clone()),
		Color::Rgba(_) => Some(color.clone()),
	}
}

fn length(value: Length) -> Length {
	match value {
		Length::Px(px) => Length::Px(px.min(MAX_PX)),
		other => other,
	}
}

fn style(style: &Style) -> Style {
	Style {
		width: length(style.width),
		height: length(style.height),
		padding: style.padding,
		gap: style.gap,
		align: style.align,
		justify: style.justify,
		background: style.background.as_ref().and_then(sanitize_color),
		border: style.border.as_ref().and_then(sanitize_color),
		radius: style.radius,
		tooltip: style.tooltip.as_deref().map(|tip| clip_line(tip, MAX_TOOLTIP)),
		disabled: style.disabled,
	}
}

fn emoji(emoji: &EmojiRef) -> Option<EmojiRef> {
	let name = clip_line(&emoji.name, 64);
	(emoji.id.is_some() || !name.is_empty()).then_some(EmojiRef {
		id: emoji.id,
		name,
		animated: emoji.animated,
	})
}

pub fn sanitize_card(card: &EmbedCard, images: &ImagePolicy) -> EmbedCard {
	EmbedCard {
		title: card.title.as_deref().map(|title| clip_line(title, 256)),
		description: card.description.as_deref().map(|text| clip(text, MAX_MARKDOWN)),
		url: card.url.clone().filter(|url| web_url(url)),
		accent: card.accent.as_ref().and_then(sanitize_color),
		image: card.image.clone().filter(|url| images.allows(url)),
		footer: card.footer.as_deref().map(|footer| clip_line(footer, 256)),
	}
}

pub fn sanitize_view(tree: &ViewTree, images: &ImagePolicy) -> ViewTree {
	let mut builder = Builder {
		source: &tree.nodes,
		out: Vec::new(),
		images,
	};
	let roots = builder.children(&tree.roots, None, 0);
	ViewTree {
		nodes: builder.out,
		roots,
	}
}

struct Builder<'a> {
	source: &'a [ViewNode],
	out: Vec<ViewNode>,
	images: &'a ImagePolicy,
}

impl Builder<'_> {
	fn children(&mut self, indices: &[u32], parent: Option<usize>, depth: usize) -> Vec<u32> {
		indices
			.iter()
			.take(MAX_CHILDREN)
			.filter_map(|&index| self.visit(index as usize, parent, depth))
			.collect()
	}

	fn visit(&mut self, index: usize, parent: Option<usize>, depth: usize) -> Option<u32> {
		if index >= self.source.len()
			|| parent.is_some_and(|parent| index <= parent)
			|| depth > MAX_VIEW_DEPTH
			|| self.out.len() >= MAX_VIEW_NODES
		{
			return None;
		}
		let node = &self.source[index];
		let slot = self.out.len();
		self.out.push(ViewNode {
			key: None,
			style: None,
			kind: NodeKind::Spacer,
		});
		match self.kind(&node.kind, index, depth + 1) {
			Some(kind) => {
				self.out[slot] = ViewNode {
					key: node.key.clone().filter(|key| valid_key(key)),
					style: node.style.as_ref().map(style),
					kind,
				};
				Some(slot as u32)
			}
			None => {
				self.out.truncate(slot);
				None
			}
		}
	}

	fn kind(&mut self, kind: &NodeKind, index: usize, depth: usize) -> Option<NodeKind> {
		let parent = Some(index);
		Some(match kind {
			NodeKind::Stack(stack) => NodeKind::Stack(StackNode {
				axis: stack.axis,
				wrap: stack.wrap,
				children: self.children(&stack.children, parent, depth),
			}),
			NodeKind::Scroll(children) => NodeKind::Scroll(self.children(children, parent, depth)),
			NodeKind::Form(children) => NodeKind::Form(self.children(children, parent, depth)),
			NodeKind::Popover(children) => {
				let children = self.children(children, parent, depth);
				if children.is_empty() {
					return None;
				}
				NodeKind::Popover(children)
			}
			NodeKind::Section(section) => NodeKind::Section(SectionNode {
				title: clip_line(&section.title, MAX_LABEL),
				collapsed: section.collapsed,
				children: self.children(&section.children, parent, depth),
			}),
			NodeKind::Tabs(tabs) => {
				let mut labels = Vec::new();
				let mut children = Vec::new();
				for (label, child) in tabs.labels.iter().zip(&tabs.children).take(MAX_TABS) {
					if let Some(child) = self.visit(*child as usize, parent, depth) {
						labels.push(clip_line(label, 64));
						children.push(child);
					}
				}
				if children.is_empty() {
					return None;
				}
				NodeKind::Tabs(TabsNode {
					selected: tabs.selected.min(children.len() as u32 - 1),
					labels,
					children,
				})
			}
			NodeKind::Spacer => NodeKind::Spacer,
			NodeKind::Divider => NodeKind::Divider,
			NodeKind::Text(text) => NodeKind::Text(TextNode {
				value: clip(&text.value, MAX_TEXT),
				color: text.color.as_ref().and_then(sanitize_color),
				..text.clone()
			}),
			NodeKind::Markdown(source) => NodeKind::Markdown(clip(source, MAX_MARKDOWN)),
			NodeKind::Image(image) => NodeKind::Image(ImageNode {
				source: match &image.source {
					ImageSource::Url(url) if self.images.allows(url) => ImageSource::Url(url.clone()),
					ImageSource::Url(_) => return None,
					ImageSource::Emoji(reference) => ImageSource::Emoji(emoji(reference)?),
					ImageSource::Avatar(user) => ImageSource::Avatar(*user),
				},
				width: image.width.min(MAX_IMAGE_SIZE),
				height: image.height.min(MAX_IMAGE_SIZE),
				alt: clip_line(&image.alt, 256),
			}),
			NodeKind::Icon(icon) if valid_icon(&icon.name) => NodeKind::Icon(IconNode {
				size: icon.size.min(64),
				..icon.clone()
			}),
			NodeKind::Icon(_) => return None,
			NodeKind::Avatar(avatar) => NodeKind::Avatar(crate::api::model::AvatarNode {
				user_id: avatar.user_id,
				size: avatar.size.clamp(16, 128),
			}),
			NodeKind::Emoji(node) => NodeKind::Emoji(EmojiNode {
				emoji: emoji(&node.emoji)?,
				size: node.size.min(128),
			}),
			NodeKind::Button(button) => NodeKind::Button(ButtonNode {
				label: clip_line(&button.label, MAX_LABEL),
				icon: button.icon.clone().filter(|icon| valid_icon(icon)),
				..button.clone()
			}),
			NodeKind::IconButton(button) if valid_icon(&button.icon) => NodeKind::IconButton(button.clone()),
			NodeKind::IconButton(_) => return None,
			NodeKind::Link(link) if web_url(&link.url) => NodeKind::Link(LinkNode {
				label: clip_line(&link.label, MAX_LABEL),
				url: link.url.clone(),
			}),
			NodeKind::Link(_) => return None,
			NodeKind::Input(input) => NodeKind::Input(input_node(input)),
			NodeKind::Textarea(input) => NodeKind::Textarea(input_node(input)),
			NodeKind::Select(select) => {
				let options: Vec<SelectOption> = select
					.options
					.iter()
					.take(MAX_OPTIONS)
					.map(|option| SelectOption {
						value: clip_line(&option.value, MAX_LABEL),
						label: clip_line(&option.label, MAX_LABEL),
					})
					.collect();
				let selected = select
					.selected
					.clone()
					.filter(|value| options.iter().any(|option| option.value == *value));
				NodeKind::Select(SelectNode {
					options,
					selected,
					placeholder: clip_line(&select.placeholder, MAX_LABEL),
				})
			}
			NodeKind::Toggle(check) => NodeKind::Toggle(check_node(check)),
			NodeKind::Checkbox(check) => NodeKind::Checkbox(check_node(check)),
			NodeKind::Slider(slider) => NodeKind::Slider(slider_node(slider)?),
			NodeKind::Progress(value) => NodeKind::Progress(if value.is_finite() {
				value.clamp(0., 1.)
			} else {
				0.
			}),
			NodeKind::Badge(badge) => NodeKind::Badge(BadgeNode {
				label: clip_line(&badge.label, 64),
				tone: badge.tone,
			}),
			NodeKind::Embed(card) => NodeKind::Embed(sanitize_card(card, self.images)),
			NodeKind::UserChip(user) => NodeKind::UserChip(*user),
			NodeKind::ChannelChip(channel) => NodeKind::ChannelChip(*channel),
			NodeKind::Timestamp(timestamp) => NodeKind::Timestamp(TimestampNode {
				unix_ms: timestamp.unix_ms,
				style: super::md::style_code(super::md::style_from_code(&timestamp.style)).to_owned(),
			}),
		})
	}
}

fn input_node(input: &InputNode) -> InputNode {
	InputNode {
		value: clip(&input.value, MAX_TEXT),
		placeholder: clip_line(&input.placeholder, MAX_TOOLTIP),
	}
}

fn check_node(check: &CheckNode) -> CheckNode {
	CheckNode {
		label: clip_line(&check.label, MAX_LABEL),
		checked: check.checked,
	}
}

fn slider_node(slider: &SliderNode) -> Option<SliderNode> {
	let finite = [slider.min, slider.max, slider.step, slider.value]
		.iter()
		.all(|value| value.is_finite());
	(finite && slider.min < slider.max && slider.step > 0. && slider.step <= slider.max - slider.min).then(
		|| SliderNode {
			value: slider.value.clamp(slider.min, slider.max),
			..*slider
		},
	)
}

pub fn parse_legacy_color(text: &str) -> Option<u32> {
	let text = text.trim();
	if let Some(hex) = text.strip_prefix('#') {
		let expand = |digit: u8| (digit << 4) | digit;
		let digits: Vec<u8> = hex
			.chars()
			.map(|character| character.to_digit(16).map(|digit| digit as u8))
			.collect::<Option<_>>()?;
		let rgba: [u8; 4] = match digits.as_slice() {
			[r, g, b] => [expand(*r), expand(*g), expand(*b), 255],
			[r, g, b, a] => [expand(*r), expand(*g), expand(*b), expand(*a)],
			[r1, r2, g1, g2, b1, b2] => [r1 << 4 | r2, g1 << 4 | g2, b1 << 4 | b2, 255],
			[r1, r2, g1, g2, b1, b2, a1, a2] => [r1 << 4 | r2, g1 << 4 | g2, b1 << 4 | b2, a1 << 4 | a2],
			_ => return None,
		};
		return Some(u32::from_be_bytes(rgba));
	}
	let body = text
		.strip_prefix("rgba(")
		.or_else(|| text.strip_prefix("rgb("))?
		.strip_suffix(')')?;
	let mut parts = body.split(',').map(str::trim);
	let mut channel = || -> Option<u8> {
		parts
			.next()?
			.parse::<f32>()
			.ok()
			.map(|value| value.clamp(0., 255.) as u8)
	};
	let (red, green, blue) = (channel()?, channel()?, channel()?);
	let alpha = parts
		.next()
		.and_then(|alpha| alpha.parse::<f32>().ok())
		.map_or(255, |alpha| (alpha.clamp(0., 1.) * 255.).round() as u8);
	Some(u32::from_be_bytes([red, green, blue, alpha]))
}

fn nearest_space(px: u8) -> Space {
	match px {
		0 => Space::None,
		1..=2 => Space::Xs,
		3..=5 => Space::Sm,
		6..=11 => Space::Md,
		12..=19 => Space::Lg,
		_ => Space::Xl,
	}
}

fn legacy_text_size(px: u8) -> TextSize {
	match px {
		0 => TextSize::Sm,
		1..=10 => TextSize::Tiny,
		11..=12 => TextSize::Xs,
		13..=14 => TextSize::Sm,
		15..=17 => TextSize::Md,
		18..=22 => TextSize::Lg,
		_ => TextSize::Xl,
	}
}

pub fn default_style() -> Style {
	Style {
		width: Length::Auto,
		height: Length::Auto,
		padding: Space::None,
		gap: Space::None,
		align: Align::Start,
		justify: Justify::Start,
		background: None,
		border: None,
		radius: Radius::None,
		tooltip: None,
		disabled: false,
	}
}

fn legacy_stack(layout: &UiBox, axis: Axis) -> ViewNode {
	let (align, justify) = match (axis, layout.align.as_str()) {
		(Axis::Row, "center") => (Align::Center, Justify::Center),
		(Axis::Row, "end") => (Align::End, Justify::End),
		(Axis::Row, "between") => (Align::Center, Justify::Between),
		(Axis::Row, "start") => (Align::Start, Justify::Start),
		(Axis::Row, _) => (Align::Center, Justify::Start),
		(Axis::Column, "center") => (Align::Center, Justify::Start),
		(Axis::Column, "end") => (Align::End, Justify::Start),
		(Axis::Column, "stretch") => (Align::Stretch, Justify::Start),
		(Axis::Column, _) => (Align::Start, Justify::Start),
	};
	ViewNode {
		key: layout.id.clone(),
		style: Some(Style {
			padding: nearest_space(layout.padding),
			gap: nearest_space(layout.gap),
			align,
			justify,
			..default_style()
		}),
		kind: NodeKind::Stack(StackNode {
			axis,
			wrap: false,
			children: layout.children.clone(),
		}),
	}
}

fn legacy_text(text: &UiText) -> NodeKind {
	let style = &text.style;
	NodeKind::Text(TextNode {
		value: text.value.clone(),
		size: legacy_text_size(style.size),
		weight: if style.bold {
			Weight::Semibold
		} else {
			Weight::Normal
		},
		tone: if style.muted { Tone::Muted } else { Tone::Normal },
		color: style
			.color
			.as_deref()
			.and_then(parse_legacy_color)
			.map(Color::Rgba),
		mono: false,
		italic: false,
	})
}

fn plain(kind: NodeKind) -> ViewNode {
	ViewNode {
		key: None,
		style: None,
		kind,
	}
}

fn keyed(key: &str, kind: NodeKind) -> ViewNode {
	ViewNode {
		key: Some(key.to_owned()),
		style: None,
		kind,
	}
}

pub fn upgrade_ui_tree(tree: &UiTree) -> ViewTree {
	let nodes = tree
		.nodes
		.iter()
		.map(|node| match node {
			UiNode::Column(layout) => legacy_stack(layout, Axis::Column),
			UiNode::Row(layout) => legacy_stack(layout, Axis::Row),
			UiNode::Text(text) => plain(legacy_text(text)),
			UiNode::Button(button) => keyed(
				&button.id,
				NodeKind::Button(ButtonNode {
					label: button.label.clone(),
					icon: button.icon.clone(),
					tone: match button.variant.as_str() {
						"primary" => Tone::Brand,
						"danger" => Tone::Danger,
						_ => Tone::Normal,
					},
					variant: match button.variant.as_str() {
						"ghost" => ButtonVariant::Ghost,
						"outline" => ButtonVariant::Outline,
						_ => ButtonVariant::Solid,
					},
					submit: false,
				}),
			),
			UiNode::Input(input) => keyed(
				&input.id,
				NodeKind::Input(InputNode {
					value: input.value.clone(),
					placeholder: input.placeholder.clone(),
				}),
			),
			UiNode::Toggle(toggle) => keyed(
				&toggle.id,
				NodeKind::Toggle(CheckNode {
					label: toggle.label.clone(),
					checked: toggle.on,
				}),
			),
			UiNode::Image(image) => plain(NodeKind::Image(ImageNode {
				source: ImageSource::Url(image.url.clone()),
				width: image.width,
				height: image.height,
				alt: String::new(),
			})),
			UiNode::Badge(badge) => ViewNode {
				key: None,
				style: badge
					.color
					.as_deref()
					.and_then(parse_legacy_color)
					.map(|rgba| Style {
						background: Some(Color::Rgba(rgba)),
						..default_style()
					}),
				kind: NodeKind::Badge(BadgeNode {
					label: badge.label.clone(),
					tone: Tone::Brand,
				}),
			},
			UiNode::Divider => plain(NodeKind::Divider),
			UiNode::Icon(name) => plain(NodeKind::Icon(IconNode {
				name: name.clone(),
				tone: Tone::Normal,
				size: 20,
			})),
			UiNode::Progress(fraction) => plain(NodeKind::Progress(*fraction)),
			UiNode::Link(link) => plain(NodeKind::Link(LinkNode {
				label: link.label.clone(),
				url: link.url.clone(),
			})),
		})
		.collect();
	ViewTree {
		nodes,
		roots: tree.roots.clone(),
	}
}

pub fn has_key(tree: &ViewTree, key: &str) -> bool {
	tree.nodes.iter().any(|node| node.key.as_deref() == Some(key))
}
