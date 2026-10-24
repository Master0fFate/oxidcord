use crate::md::{Color, api_color};
use crate::model::{
	Align, AvatarNode, Axis, BadgeNode, ButtonNode, ButtonVariant, CheckNode, EmbedCard, EmojiNode, EmojiRef,
	IconButtonNode, IconNode, ImageNode, ImageSource, InputNode, Justify, Length, LinkNode, NodeKind, Radius,
	SectionNode, SelectNode, SelectOption, SliderNode, Space, StackNode, Style, TabsNode, TextNode, TextSize,
	TimestampNode, Tone, ViewNode, ViewTree, Weight,
};

#[derive(Debug, Clone, PartialEq)]
enum Kind {
	Stack {
		axis: Axis,
		wrap: bool,
		children: Vec<Node>,
	},
	Scroll(Vec<Node>),
	Form(Vec<Node>),
	Popover(Vec<Node>),
	Section {
		title: String,
		collapsed: bool,
		children: Vec<Node>,
	},
	Tabs {
		labels: Vec<String>,
		selected: u32,
		children: Vec<Node>,
	},
	Leaf(NodeKind),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
	key: Option<String>,
	style: Option<Style>,
	kind: Kind,
}

fn base_style() -> Style {
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

fn node(kind: Kind) -> Node {
	Node {
		key: None,
		style: None,
		kind,
	}
}

fn leaf(kind: NodeKind) -> Node {
	node(Kind::Leaf(kind))
}

impl Node {
	fn style_mut(&mut self) -> &mut Style {
		self.style.get_or_insert_with(base_style)
	}

	fn styled(mut self, edit: impl FnOnce(&mut Style)) -> Self {
		edit(self.style_mut());
		self
	}

	pub fn key(mut self, key: impl Into<String>) -> Self {
		self.key = Some(key.into());
		self
	}

	pub fn width(self, width: Length) -> Self {
		self.styled(|style| style.width = width)
	}

	pub fn height(self, height: Length) -> Self {
		self.styled(|style| style.height = height)
	}

	pub fn fill(self) -> Self {
		self.styled(|style| style.width = Length::Fill)
	}

	pub fn padding(self, padding: Space) -> Self {
		self.styled(|style| style.padding = padding)
	}

	pub fn gap(self, gap: Space) -> Self {
		self.styled(|style| style.gap = gap)
	}

	pub fn align(self, align: Align) -> Self {
		self.styled(|style| style.align = align)
	}

	pub fn justify(self, justify: Justify) -> Self {
		self.styled(|style| style.justify = justify)
	}

	pub fn background(self, color: Color) -> Self {
		self.styled(|style| style.background = Some(api_color(&color)))
	}

	pub fn border(self, color: Color) -> Self {
		self.styled(|style| style.border = Some(api_color(&color)))
	}

	pub fn radius(self, radius: Radius) -> Self {
		self.styled(|style| style.radius = radius)
	}

	pub fn tooltip(self, tooltip: impl Into<String>) -> Self {
		let tooltip = tooltip.into();
		self.styled(|style| style.tooltip = Some(tooltip))
	}

	pub fn disabled(self, disabled: bool) -> Self {
		self.styled(|style| style.disabled = disabled)
	}

	pub fn child(mut self, child: impl Into<Node>) -> Self {
		if let Some(children) = self.children_mut() {
			children.push(child.into());
		}
		self
	}

	pub fn children(mut self, children: impl IntoIterator<Item = impl Into<Node>>) -> Self {
		if let Some(list) = self.children_mut() {
			list.extend(children.into_iter().map(Into::into));
		}
		self
	}

	pub fn tab(mut self, label: impl Into<String>, content: impl Into<Node>) -> Self {
		if let Kind::Tabs { labels, children, .. } = &mut self.kind {
			labels.push(label.into());
			children.push(content.into());
		}
		self
	}

	pub fn selected(mut self, index: u32) -> Self {
		if let Kind::Tabs { selected, .. } = &mut self.kind {
			*selected = index;
		}
		self
	}

	pub fn wrap(mut self) -> Self {
		if let Kind::Stack { wrap, .. } = &mut self.kind {
			*wrap = true;
		}
		self
	}

	pub fn collapsed(mut self, value: bool) -> Self {
		if let Kind::Section { collapsed, .. } = &mut self.kind {
			*collapsed = value;
		}
		self
	}

	fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
		match &mut self.kind {
			Kind::Stack { children, .. }
			| Kind::Scroll(children)
			| Kind::Form(children)
			| Kind::Popover(children)
			| Kind::Section { children, .. } => Some(children),
			Kind::Tabs { .. } | Kind::Leaf(_) => None,
		}
	}

	pub fn size(mut self, size: TextSize) -> Self {
		if let Kind::Leaf(NodeKind::Text(text)) = &mut self.kind {
			text.size = size;
		}
		self
	}

	pub fn weight(mut self, weight: Weight) -> Self {
		if let Kind::Leaf(NodeKind::Text(text)) = &mut self.kind {
			text.weight = weight;
		}
		self
	}

	pub fn bold(self) -> Self {
		self.weight(Weight::Semibold)
	}

	pub fn muted(self) -> Self {
		self.tone(Tone::Muted)
	}

	pub fn mono(mut self) -> Self {
		if let Kind::Leaf(NodeKind::Text(text)) = &mut self.kind {
			text.mono = true;
		}
		self
	}

	pub fn italic(mut self) -> Self {
		if let Kind::Leaf(NodeKind::Text(text)) = &mut self.kind {
			text.italic = true;
		}
		self
	}

	pub fn color(mut self, color: Color) -> Self {
		if let Kind::Leaf(NodeKind::Text(text)) = &mut self.kind {
			text.color = Some(api_color(&color));
		}
		self
	}

	pub fn tone(mut self, tone: Tone) -> Self {
		if let Kind::Leaf(kind) = &mut self.kind {
			match kind {
				NodeKind::Text(text) => text.tone = tone,
				NodeKind::Button(button) => button.tone = tone,
				NodeKind::IconButton(button) => button.tone = tone,
				NodeKind::Icon(icon) => icon.tone = tone,
				NodeKind::Badge(badge) => badge.tone = tone,
				_ => {}
			}
		}
		self
	}

	pub fn icon(mut self, name: impl Into<String>) -> Self {
		if let Kind::Leaf(NodeKind::Button(button)) = &mut self.kind {
			button.icon = Some(name.into());
		}
		self
	}

	pub fn variant(mut self, variant: ButtonVariant) -> Self {
		if let Kind::Leaf(NodeKind::Button(button)) = &mut self.kind {
			button.variant = variant;
		}
		self
	}

	pub fn submit(mut self) -> Self {
		if let Kind::Leaf(NodeKind::Button(button)) = &mut self.kind {
			button.submit = true;
		}
		self
	}

	pub fn value(mut self, value: impl Into<String>) -> Self {
		if let Kind::Leaf(NodeKind::Input(input) | NodeKind::Textarea(input)) = &mut self.kind {
			input.value = value.into();
		}
		self
	}

	pub fn option(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
		if let Kind::Leaf(NodeKind::Select(select)) = &mut self.kind {
			select.options.push(SelectOption {
				value: value.into(),
				label: label.into(),
			});
		}
		self
	}

	pub fn choose(mut self, value: impl Into<String>) -> Self {
		if let Kind::Leaf(NodeKind::Select(select)) = &mut self.kind {
			select.selected = Some(value.into());
		}
		self
	}

	pub fn image_size(mut self, width: u32, height: u32) -> Self {
		if let Kind::Leaf(NodeKind::Image(image)) = &mut self.kind {
			image.width = width;
			image.height = height;
		}
		self
	}
}

pub fn column() -> Node {
	node(Kind::Stack {
		axis: Axis::Column,
		wrap: false,
		children: Vec::new(),
	})
}

pub fn row() -> Node {
	node(Kind::Stack {
		axis: Axis::Row,
		wrap: false,
		children: Vec::new(),
	})
	.align(Align::Center)
}

pub fn scroll() -> Node {
	node(Kind::Scroll(Vec::new()))
}

pub fn form() -> Node {
	node(Kind::Form(Vec::new()))
}

pub fn popover(trigger: impl Into<Node>) -> Node {
	node(Kind::Popover(vec![trigger.into()]))
}

pub fn section(title: impl Into<String>) -> Node {
	node(Kind::Section {
		title: title.into(),
		collapsed: false,
		children: Vec::new(),
	})
}

pub fn tabs() -> Node {
	node(Kind::Tabs {
		labels: Vec::new(),
		selected: 0,
		children: Vec::new(),
	})
}

pub fn spacer() -> Node {
	leaf(NodeKind::Spacer)
}

pub fn divider() -> Node {
	leaf(NodeKind::Divider)
}

pub fn text(value: impl Into<String>) -> Node {
	leaf(NodeKind::Text(TextNode {
		value: value.into(),
		size: TextSize::Sm,
		weight: Weight::Normal,
		tone: Tone::Normal,
		color: None,
		mono: false,
		italic: false,
	}))
}

pub fn heading(value: impl Into<String>) -> Node {
	text(value).size(TextSize::Lg).weight(Weight::Bold)
}

pub fn markdown(source: impl Into<String>) -> Node {
	leaf(NodeKind::Markdown(source.into()))
}

pub fn image(url: impl Into<String>) -> Node {
	leaf(NodeKind::Image(ImageNode {
		source: ImageSource::Url(url.into()),
		width: 0,
		height: 0,
		alt: String::new(),
	}))
}

pub fn icon(name: impl Into<String>) -> Node {
	leaf(NodeKind::Icon(IconNode {
		name: name.into(),
		tone: Tone::Normal,
		size: 0,
	}))
}

pub fn avatar(user_id: u64, size: u32) -> Node {
	leaf(NodeKind::Avatar(AvatarNode { user_id, size }))
}

pub fn emoji(emoji: EmojiRef, size: u32) -> Node {
	leaf(NodeKind::Emoji(EmojiNode { emoji, size }))
}

pub fn button(key: impl Into<String>, label: impl Into<String>) -> Node {
	leaf(NodeKind::Button(ButtonNode {
		label: label.into(),
		icon: None,
		tone: Tone::Normal,
		variant: ButtonVariant::Solid,
		submit: false,
	}))
	.key(key)
}

pub fn icon_button(key: impl Into<String>, icon: impl Into<String>, tooltip: impl Into<String>) -> Node {
	leaf(NodeKind::IconButton(IconButtonNode {
		icon: icon.into(),
		tone: Tone::Normal,
	}))
	.key(key)
	.tooltip(tooltip)
}

pub fn link(label: impl Into<String>, url: impl Into<String>) -> Node {
	leaf(NodeKind::Link(LinkNode {
		label: label.into(),
		url: url.into(),
	}))
}

pub fn input(key: impl Into<String>, placeholder: impl Into<String>) -> Node {
	leaf(NodeKind::Input(InputNode {
		value: String::new(),
		placeholder: placeholder.into(),
	}))
	.key(key)
}

pub fn textarea(key: impl Into<String>, placeholder: impl Into<String>) -> Node {
	leaf(NodeKind::Textarea(InputNode {
		value: String::new(),
		placeholder: placeholder.into(),
	}))
	.key(key)
}

pub fn select(key: impl Into<String>, placeholder: impl Into<String>) -> Node {
	leaf(NodeKind::Select(SelectNode {
		options: Vec::new(),
		selected: None,
		placeholder: placeholder.into(),
	}))
	.key(key)
}

pub fn toggle(key: impl Into<String>, label: impl Into<String>, checked: bool) -> Node {
	leaf(NodeKind::Toggle(CheckNode {
		label: label.into(),
		checked,
	}))
	.key(key)
}

pub fn checkbox(key: impl Into<String>, label: impl Into<String>, checked: bool) -> Node {
	leaf(NodeKind::Checkbox(CheckNode {
		label: label.into(),
		checked,
	}))
	.key(key)
}

pub fn slider(key: impl Into<String>, min: f32, max: f32, step: f32, value: f32) -> Node {
	leaf(NodeKind::Slider(SliderNode {
		min,
		max,
		step,
		value,
	}))
	.key(key)
}

pub fn progress(fraction: f32) -> Node {
	leaf(NodeKind::Progress(fraction))
}

pub fn badge(label: impl Into<String>) -> Node {
	leaf(NodeKind::Badge(BadgeNode {
		label: label.into(),
		tone: Tone::Brand,
	}))
}

pub fn embed(card: EmbedCard) -> Node {
	leaf(NodeKind::Embed(card))
}

pub fn user_chip(user_id: u64) -> Node {
	leaf(NodeKind::UserChip(user_id))
}

pub fn channel_chip(channel_id: u64) -> Node {
	leaf(NodeKind::ChannelChip(channel_id))
}

pub fn timestamp(unix_ms: i64, style: impl Into<String>) -> Node {
	leaf(NodeKind::Timestamp(TimestampNode {
		unix_ms,
		style: style.into(),
	}))
}

pub fn card(title: impl Into<String>) -> EmbedCard {
	EmbedCard {
		title: Some(title.into()),
		description: None,
		url: None,
		accent: None,
		image: None,
		footer: None,
	}
}

pub fn tree<N: Into<Node>>(roots: impl IntoIterator<Item = N>) -> ViewTree {
	let mut nodes = Vec::new();
	let roots = roots
		.into_iter()
		.map(|root| push(&mut nodes, root.into()))
		.collect();
	ViewTree { nodes, roots }
}

fn push_all(out: &mut Vec<ViewNode>, children: Vec<Node>) -> Vec<u32> {
	children.into_iter().map(|child| push(out, child)).collect()
}

fn push(out: &mut Vec<ViewNode>, node: Node) -> u32 {
	let index = out.len();
	out.push(ViewNode {
		key: None,
		style: None,
		kind: NodeKind::Spacer,
	});
	let kind = match node.kind {
		Kind::Stack { axis, wrap, children } => NodeKind::Stack(StackNode {
			axis,
			wrap,
			children: push_all(out, children),
		}),
		Kind::Scroll(children) => NodeKind::Scroll(push_all(out, children)),
		Kind::Form(children) => NodeKind::Form(push_all(out, children)),
		Kind::Popover(children) => NodeKind::Popover(push_all(out, children)),
		Kind::Section {
			title,
			collapsed,
			children,
		} => NodeKind::Section(SectionNode {
			title,
			collapsed,
			children: push_all(out, children),
		}),
		Kind::Tabs {
			labels,
			selected,
			children,
		} => NodeKind::Tabs(TabsNode {
			labels,
			selected,
			children: push_all(out, children),
		}),
		Kind::Leaf(kind) => kind,
	};
	out[index] = ViewNode {
		key: node.key,
		style: node.style,
		kind,
	};
	index as u32
}
