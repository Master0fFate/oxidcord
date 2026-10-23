use crate::host;
use crate::types::{
	HostError, TextStyle, UiBadge, UiBox, UiButton, UiImage, UiInput, UiLink, UiNode, UiText, UiToggle,
	UiTree,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Ui {
	Stack(Stack),
	Text(Text),
	Button(Button),
	Input(Input),
	Toggle(Toggle),
	Image(Image),
	Badge(Badge),
	Divider,
	Icon(String),
	Progress(f32),
	Link { label: String, url: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
	Column,
	Row,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stack {
	axis: Axis,
	id: Option<String>,
	gap: u8,
	padding: u8,
	align: String,
	children: Vec<Ui>,
}

pub fn column() -> Stack {
	Stack::new(Axis::Column)
}

pub fn row() -> Stack {
	Stack::new(Axis::Row)
}

impl Stack {
	fn new(axis: Axis) -> Self {
		Self {
			axis,
			id: None,
			gap: 4,
			padding: 0,
			align: "start".to_owned(),
			children: Vec::new(),
		}
	}

	pub fn id(mut self, id: impl Into<String>) -> Self {
		self.id = Some(id.into());
		self
	}

	pub fn gap(mut self, gap: u8) -> Self {
		self.gap = gap;
		self
	}

	pub fn padding(mut self, padding: u8) -> Self {
		self.padding = padding;
		self
	}

	pub fn align(mut self, align: impl Into<String>) -> Self {
		self.align = align.into();
		self
	}

	pub fn child(mut self, child: impl Into<Ui>) -> Self {
		self.children.push(child.into());
		self
	}

	pub fn button(self, id: impl Into<String>, label: impl Into<String>) -> Self {
		self.child(button(id, label))
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
	value: String,
	bold: bool,
	muted: bool,
	size: u8,
	color: Option<String>,
}

pub fn text(value: impl Into<String>) -> Text {
	Text {
		value: value.into(),
		bold: false,
		muted: false,
		size: 0,
		color: None,
	}
}

impl Text {
	pub fn bold(mut self) -> Self {
		self.bold = true;
		self
	}

	pub fn muted(mut self) -> Self {
		self.muted = true;
		self
	}

	pub fn size(mut self, size: u8) -> Self {
		self.size = size;
		self
	}

	pub fn color(mut self, color: impl Into<String>) -> Self {
		self.color = Some(color.into());
		self
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Button {
	id: String,
	label: String,
	variant: String,
	icon: Option<String>,
}

pub fn button(id: impl Into<String>, label: impl Into<String>) -> Button {
	Button {
		id: id.into(),
		label: label.into(),
		variant: "default".to_owned(),
		icon: None,
	}
}

impl Button {
	pub fn variant(mut self, variant: impl Into<String>) -> Self {
		self.variant = variant.into();
		self
	}

	pub fn icon(mut self, icon: impl Into<String>) -> Self {
		self.icon = Some(icon.into());
		self
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Input {
	id: String,
	placeholder: String,
	value: String,
}

pub fn input(id: impl Into<String>, placeholder: impl Into<String>) -> Input {
	Input {
		id: id.into(),
		placeholder: placeholder.into(),
		value: String::new(),
	}
}

impl Input {
	pub fn value(mut self, value: impl Into<String>) -> Self {
		self.value = value.into();
		self
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toggle {
	id: String,
	label: String,
	on: bool,
}

pub fn toggle(id: impl Into<String>, label: impl Into<String>, on: bool) -> Toggle {
	Toggle {
		id: id.into(),
		label: label.into(),
		on,
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
	url: String,
	width: u32,
	height: u32,
}

pub fn image(url: impl Into<String>, width: u32, height: u32) -> Image {
	Image {
		url: url.into(),
		width,
		height,
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
	label: String,
	color: Option<String>,
}

pub fn badge(label: impl Into<String>) -> Badge {
	Badge {
		label: label.into(),
		color: None,
	}
}

impl Badge {
	pub fn color(mut self, color: impl Into<String>) -> Self {
		self.color = Some(color.into());
		self
	}
}

pub fn divider() -> Ui {
	Ui::Divider
}

pub fn icon(name: impl Into<String>) -> Ui {
	Ui::Icon(name.into())
}

pub fn progress(fraction: f32) -> Ui {
	Ui::Progress(fraction)
}

pub fn link(label: impl Into<String>, url: impl Into<String>) -> Ui {
	Ui::Link {
		label: label.into(),
		url: url.into(),
	}
}

macro_rules! into_ui {
	($($builder:ident => $variant:ident),* $(,)?) => {
		$(impl From<$builder> for Ui {
			fn from(value: $builder) -> Self {
				Ui::$variant(value)
			}
		})*
	};
}

into_ui!(
	Stack => Stack,
	Text => Text,
	Button => Button,
	Input => Input,
	Toggle => Toggle,
	Image => Image,
	Badge => Badge,
);

pub fn to_tree(roots: &[Ui]) -> UiTree {
	let mut nodes = Vec::new();
	let roots = push_all(&mut nodes, roots);
	UiTree { nodes, roots }
}

fn push_all(out: &mut Vec<UiNode>, items: &[Ui]) -> Vec<u32> {
	items.iter().map(|item| push(out, item)).collect()
}

fn push(out: &mut Vec<UiNode>, item: &Ui) -> u32 {
	let index = out.len();
	out.push(UiNode::Divider);
	let node = match item {
		Ui::Stack(stack) => {
			let layout = UiBox {
				id: stack.id.clone(),
				gap: stack.gap,
				padding: stack.padding,
				children: push_all(out, &stack.children),
				align: stack.align.clone(),
			};
			match stack.axis {
				Axis::Column => UiNode::Column(layout),
				Axis::Row => UiNode::Row(layout),
			}
		}
		Ui::Text(text) => UiNode::Text(UiText {
			value: text.value.clone(),
			style: TextStyle {
				bold: text.bold,
				muted: text.muted,
				size: text.size,
				color: text.color.clone(),
			},
		}),
		Ui::Button(button) => UiNode::Button(UiButton {
			id: button.id.clone(),
			label: button.label.clone(),
			variant: button.variant.clone(),
			icon: button.icon.clone(),
		}),
		Ui::Input(input) => UiNode::Input(UiInput {
			id: input.id.clone(),
			placeholder: input.placeholder.clone(),
			value: input.value.clone(),
		}),
		Ui::Toggle(toggle) => UiNode::Toggle(UiToggle {
			id: toggle.id.clone(),
			label: toggle.label.clone(),
			on: toggle.on,
		}),
		Ui::Image(image) => UiNode::Image(UiImage {
			url: image.url.clone(),
			width: image.width,
			height: image.height,
		}),
		Ui::Badge(badge) => UiNode::Badge(UiBadge {
			label: badge.label.clone(),
			color: badge.color.clone(),
		}),
		Ui::Divider => UiNode::Divider,
		Ui::Icon(name) => UiNode::Icon(name.clone()),
		Ui::Progress(fraction) => UiNode::Progress(*fraction),
		Ui::Link { label, url } => UiNode::Link(UiLink {
			label: label.clone(),
			url: url.clone(),
		}),
	};
	out[index] = node;
	index as u32
}

pub fn register_slot(slot: &str, roots: &[Ui]) -> Result<(), HostError> {
	host::register_slot(slot, &to_tree(roots))
}

pub fn register_command(name: &str, description: &str) -> Result<(), HostError> {
	host::register_command(name, description)
}
