use crate::md::{self, Color, Md, api_color};
use crate::model::{
	Annotation, AttachmentLayout, AttachmentOverride, CodeBlockNode, Decoration, EmbedAction, EmbedCard,
	MessagePatch, MessagePatchExt, Tone, Visibility,
};
use crate::view::{self, Node};

#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
	content: Option<Vec<Md>>,
	visibility: Visibility,
	annotations: Vec<Annotation>,
	decoration: Option<Decoration>,
	embeds: Vec<EmbedAction>,
	extra_embeds: Vec<EmbedCard>,
	hidden_attachments: Vec<u64>,
	accessory: Option<Vec<Node>>,
	retain: bool,
}

impl Patch {
	pub fn keep() -> Self {
		Self {
			content: None,
			visibility: Visibility::Shown,
			annotations: Vec::new(),
			decoration: None,
			embeds: Vec::new(),
			extra_embeds: Vec::new(),
			hidden_attachments: Vec::new(),
			accessory: None,
			retain: false,
		}
	}

	pub fn content(mut self, content: Vec<Md>) -> Self {
		self.content = Some(content);
		self
	}

	pub fn hide(mut self, reason: impl Into<String>) -> Self {
		self.visibility = Visibility::Hidden(reason.into());
		self
	}

	pub fn collapse(mut self, reason: impl Into<String>) -> Self {
		self.visibility = Visibility::Collapsed(reason.into());
		self
	}

	pub fn annotate(mut self, text: impl Into<String>) -> Self {
		self.annotations.push(Annotation {
			text: text.into(),
			icon: None,
			tone: Tone::Muted,
		});
		self
	}

	pub fn annotate_with(mut self, text: impl Into<String>, icon: impl Into<String>, tone: Tone) -> Self {
		self.annotations.push(Annotation {
			text: text.into(),
			icon: Some(icon.into()),
			tone,
		});
		self
	}

	pub fn accent(mut self, color: Color) -> Self {
		self.decoration_mut().accent = Some(api_color(&color));
		self
	}

	pub fn background(mut self, color: Color) -> Self {
		self.decoration_mut().background = Some(api_color(&color));
		self
	}

	pub fn faded(mut self) -> Self {
		self.decoration_mut().faded = true;
		self
	}

	pub fn strike(mut self) -> Self {
		self.decoration_mut().strike = true;
		self
	}

	fn decoration_mut(&mut self) -> &mut Decoration {
		self.decoration.get_or_insert(Decoration {
			accent: None,
			background: None,
			faded: false,
			strike: false,
		})
	}

	pub fn embeds(mut self, actions: Vec<EmbedAction>) -> Self {
		self.embeds = actions;
		self
	}

	pub fn hide_embed(mut self, index: usize) -> Self {
		if self.embeds.len() <= index {
			self.embeds.resize(index + 1, EmbedAction::Keep);
		}
		self.embeds[index] = EmbedAction::Hide;
		self
	}

	pub fn extra_embed(mut self, card: EmbedCard) -> Self {
		self.extra_embeds.push(card);
		self
	}

	pub fn hide_attachment(mut self, id: u64) -> Self {
		self.hidden_attachments.push(id);
		self
	}

	pub fn accessory<N: Into<Node>>(mut self, roots: impl IntoIterator<Item = N>) -> Self {
		self.accessory = Some(roots.into_iter().map(Into::into).collect());
		self
	}

	pub fn retain(mut self) -> Self {
		self.retain = true;
		self
	}

	pub(crate) fn into_api(self) -> MessagePatch {
		MessagePatch {
			content: self.content.as_deref().map(md::to_rich),
			visibility: self.visibility,
			annotations: self.annotations,
			decoration: self.decoration,
			embeds: self.embeds,
			extra_embeds: self.extra_embeds,
			hidden_attachments: self.hidden_attachments,
			accessory: self.accessory.map(view::tree),
			retain: self.retain,
		}
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatchExt {
	base: Patch,
	attachments: Vec<AttachmentOverride>,
	layout: Option<AttachmentLayout>,
	reply_annotation: Option<String>,
	reveal_spoilers: bool,
	tooltip: Option<String>,
	code_blocks: Vec<(u32, CodeBlockNode)>,
}

impl PatchExt {
	pub fn keep() -> Self {
		Patch::keep().into()
	}

	pub fn base(mut self, base: Patch) -> Self {
		self.base = base;
		self
	}

	pub fn attachment(mut self, attachment: AttachmentOverride) -> Self {
		self.attachments.push(attachment);
		self
	}

	pub fn layout(mut self, layout: AttachmentLayout) -> Self {
		self.layout = Some(layout);
		self
	}

	pub fn reply_annotation(mut self, text: impl Into<String>) -> Self {
		self.reply_annotation = Some(text.into());
		self
	}

	pub fn reveal_spoilers(mut self) -> Self {
		self.reveal_spoilers = true;
		self
	}

	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}

	pub fn code_block(mut self, index: u32, block: CodeBlockNode) -> Self {
		self.code_blocks.push((index, block));
		self
	}

	pub(crate) fn into_api(self) -> MessagePatchExt {
		MessagePatchExt {
			base: self.base.into_api(),
			attachments: self.attachments,
			layout: self.layout,
			reply_annotation: self.reply_annotation,
			reveal_spoilers: self.reveal_spoilers,
			tooltip: self.tooltip,
			code_blocks: self.code_blocks,
		}
	}
}

impl From<Patch> for PatchExt {
	fn from(base: Patch) -> Self {
		Self {
			base,
			attachments: Vec::new(),
			layout: None,
			reply_annotation: None,
			reveal_spoilers: false,
			tooltip: None,
			code_blocks: Vec::new(),
		}
	}
}
