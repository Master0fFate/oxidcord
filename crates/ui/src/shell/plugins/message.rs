use gpui::{
	AnyElement, App, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
	StatefulInteractiveElement as _, Styled as _, div,
};
use plugin_host::ViewContext;
use plugin_host::api::model::{
	AttachmentLayout, AttachmentOverride, CodeBlockNode, EmbedAction, EmbedCard, NodeKind, Tone, Visibility,
};
use protocol::Snowflake;
use state::MessageView;

use super::bridge::{PatchSet, Patched, PluginBridge, Surface};
use super::chrome::compact_row;
use super::icon::PluginIcon;
use super::view::{Ctx, MESSAGE_ACTION, Scope, embed_card, resolve};
use crate::components::tooltip;
use crate::i18n::{ArgValue, Translate as _};
use crate::icons::{Icon, IconKind};
use crate::theme::{Theme, rem};

const ACCESSORY_SLOT: &str = "message-accessory";
const TARGET_PREFIX: &str = "msg-";
const CAPTION_ICON: f32 = 12.;

type Part = (usize, Option<u64>);

pub fn context_of(view: &MessageView) -> ViewContext {
	let message = &view.message;
	ViewContext {
		guild_id: message.guild_id.map(Snowflake::get),
		channel_id: Some(message.channel_id.get()),
		message_id: Some(message.id.get()),
		user_id: Some(message.author.id.get()),
		attachment_id: None,
	}
}

pub fn message_actions(view: &MessageView, cx: &App) -> Vec<AnyElement> {
	let message = view.message.id;
	compact_row(
		"message-action",
		MESSAGE_ACTION,
		message.to_string().into(),
		context_of(view),
		cx,
	)
}

fn children_of(kind: &NodeKind) -> Option<&[u32]> {
	match kind {
		NodeKind::Stack(stack) => Some(&stack.children),
		_ => None,
	}
}

fn target_of(surface: &Surface, index: usize) -> Option<u64> {
	let node = surface.tree.nodes.get(index)?;
	children_of(&node.kind)?;
	node.key.as_deref()?.strip_prefix(TARGET_PREFIX)?.parse().ok()
}

fn parts(surface: &Surface) -> Vec<Part> {
	let tree = &surface.tree;
	let mut parts = Vec::new();
	for root in &tree.roots {
		let index = *root as usize;
		let Some(children) = tree.nodes.get(index).and_then(|node| children_of(&node.kind)) else {
			parts.push((index, None));
			continue;
		};
		if let Some(target) = target_of(surface, index) {
			parts.push((index, Some(target)));
			continue;
		}
		let targeted: Vec<Part> = children
			.iter()
			.map(|child| (*child as usize, target_of(surface, *child as usize)))
			.collect();
		if targeted.iter().any(|(_, target)| target.is_some()) {
			parts.extend(targeted);
		} else {
			parts.push((index, None));
		}
	}
	parts
}

fn legacy_accessories(message: Snowflake, theme: &Theme, cx: &App) -> Vec<AnyElement> {
	let surfaces = cx.global::<PluginBridge>().slot_entries(ACCESSORY_SLOT, cx);
	let id = message.get();
	let mut out = Vec::new();
	for surface in surfaces.iter() {
		for (index, target) in parts(surface) {
			if target.is_none_or(|target| target == id) {
				let scope = Scope::slot(&surface.plugin_id, ACCESSORY_SLOT, message.to_string());
				out.push(
					div()
						.mt(theme.space_4)
						.child(Ctx::new(&surface.tree, scope).node(index, false, theme))
						.into_any_element(),
				);
			}
		}
	}
	out
}

fn caption(icon: IconKind, text: SharedString, theme: &Theme) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(theme.space_4)
		.child(Icon::new(icon).size(rem(CAPTION_ICON)).color(theme.text_muted))
		.child(text)
		.into_any_element()
}

fn attribution(view: &MessageView, theme: &Theme, cx: &App) -> Option<AnyElement> {
	if view.plugin.is_none() && view.altered_by.is_none() {
		return None;
	}
	let bridge = cx.global::<PluginBridge>();
	let via = view.plugin.as_ref().map(|id| {
		let name = bridge.name_of(id, cx);
		caption(
			IconKind::Plugins,
			cx.t_args("plugin-via", &[("name", ArgValue::from(name.to_string()))]),
			theme,
		)
	});
	let altered = view.altered_by.as_ref().map(|id| {
		let name = bridge.name_of(id, cx);
		let hint = cx.t_args("plugin-edited-by", &[("name", ArgValue::from(name.to_string()))]);
		div()
			.id(("plugin-altered", view.message.id.get()))
			.flex()
			.items_center()
			.tooltip(tooltip(hint))
			.child(
				Icon::new(IconKind::Edit)
					.size(rem(CAPTION_ICON))
					.color(theme.text_muted),
			)
	});
	Some(
		div()
			.flex()
			.items_center()
			.gap(theme.space_8)
			.mt(rem(2.))
			.text_size(theme.font_tiny)
			.text_color(theme.text_muted)
			.children(via)
			.children(altered)
			.into_any_element(),
	)
}

pub fn extras(
	view: &MessageView,
	effects: Option<&PatchEffects>,
	theme: &Theme,
	cx: &App,
) -> Vec<AnyElement> {
	let mut out = legacy_accessories(view.message.id, theme, cx);
	if let Some(effects) = effects {
		out.extend(effects.accessories(view, theme));
		out.extend(effects.annotations(theme, cx));
	}
	out.extend(attribution(view, theme, cx));
	if view.failed
		&& let Some(reason) = &view.failure_reason
	{
		out.push(
			div()
				.text_size(theme.font_sm)
				.text_color(theme.danger)
				.child(reason.clone())
				.into_any_element(),
		);
	}
	out
}

pub fn author_badge(view: &MessageView, theme: &Theme) -> Option<AnyElement> {
	let badge = view.author_badge.as_ref()?;
	Some(
		div()
			.flex_none()
			.px(theme.space_4)
			.rounded(theme.radius_sm)
			.bg(theme.mention_background)
			.text_color(theme.mention_text)
			.text_size(theme.font_tiny)
			.child(badge.clone())
			.into_any_element(),
	)
}

pub struct Veil {
	pub plugin: SharedString,
	pub reason: SharedString,
	pub collapsed: bool,
}

pub struct PatchEffects {
	set: Patched,
}

pub fn effects(message: Snowflake, cx: &App) -> Option<PatchEffects> {
	if !cx.has_global::<PluginBridge>() {
		return None;
	}
	cx.global::<PluginBridge>()
		.patches(message.get(), cx)
		.map(|set| PatchEffects { set })
}

impl PatchEffects {
	fn set(&self) -> &PatchSet {
		&self.set
	}

	pub fn veil(&self, cx: &App) -> Option<Veil> {
		self.set().patches.iter().find_map(|patch| {
			let (reason, collapsed) = match &patch.visibility {
				Visibility::Shown => return None,
				Visibility::Hidden(reason) => (reason, false),
				Visibility::Collapsed(reason) => (reason, true),
			};
			Some(Veil {
				plugin: cx.global::<PluginBridge>().name_of(&patch.plugin_id, cx),
				reason: reason.clone().into(),
				collapsed,
			})
		})
	}

	fn decoration<T>(&self, pick: impl Fn(&plugin_host::api::model::Decoration) -> Option<T>) -> Option<T> {
		self.set()
			.patches
			.iter()
			.filter_map(|patch| patch.decoration.as_ref())
			.find_map(pick)
	}

	pub fn accent(&self, theme: &Theme) -> Option<Hsla> {
		self.decoration(|decoration| decoration.accent.as_ref().and_then(|color| resolve(color, theme)))
	}

	pub fn background(&self, theme: &Theme) -> Option<Hsla> {
		self.decoration(|decoration| {
			decoration
				.background
				.as_ref()
				.and_then(|color| resolve(color, theme))
		})
	}

	pub fn faded(&self) -> bool {
		self.decoration(|decoration| decoration.faded.then_some(()))
			.is_some()
	}

	pub fn strike(&self) -> bool {
		self.decoration(|decoration| decoration.strike.then_some(()))
			.is_some()
	}

	pub fn hides_attachment(&self, id: Snowflake) -> bool {
		self.set().patches.iter().any(|patch| {
			patch.hidden_attachments.contains(&id.get())
				|| patch
					.attachments
					.iter()
					.any(|over| over.id == id.get() && over.hidden)
		})
	}

	pub fn touches_media(&self) -> bool {
		self.set().patches.iter().any(|patch| {
			!patch.hidden_attachments.is_empty()
				|| patch
					.attachments
					.iter()
					.any(|over| over.hidden || over.url.is_some())
				|| patch.embeds.iter().any(|action| *action != EmbedAction::Keep)
		})
	}

	pub fn attachment_override(&self, id: Snowflake) -> Option<&AttachmentOverride> {
		self.set()
			.patches
			.iter()
			.flat_map(|patch| patch.attachments.iter())
			.find(|over| over.id == id.get())
	}

	pub fn layout(&self) -> Option<AttachmentLayout> {
		self.set().patches.iter().find_map(|patch| patch.layout)
	}

	pub fn reply_annotation(&self) -> Option<&str> {
		self.set()
			.patches
			.iter()
			.find_map(|patch| patch.reply_annotation.as_deref())
	}

	pub fn reveals_spoilers(&self) -> bool {
		self.set().patches.iter().any(|patch| patch.reveal_spoilers)
	}

	pub fn tooltip(&self) -> Option<&str> {
		self.set()
			.patches
			.iter()
			.find_map(|patch| patch.tooltip.as_deref())
	}

	pub fn code_block(&self, index: u32) -> Option<&CodeBlockNode> {
		self.set()
			.patches
			.iter()
			.flat_map(|patch| patch.code_blocks.iter())
			.find(|(at, _)| *at == index)
			.map(|(_, block)| block)
	}

	pub fn embed_action(&self, index: usize) -> Option<&EmbedAction> {
		self.set()
			.patches
			.iter()
			.filter_map(|patch| patch.embeds.get(index))
			.find(|action| **action != EmbedAction::Keep)
	}

	pub fn replacement_cards(&self, count: usize) -> Vec<&EmbedCard> {
		let replaced = (0..count).filter_map(|index| match self.embed_action(index) {
			Some(EmbedAction::Replace(card)) => Some(card),
			_ => None,
		});
		replaced
			.chain(
				self.set()
					.patches
					.iter()
					.flat_map(|patch| patch.extra_embeds.iter()),
			)
			.collect()
	}

	pub fn cards(&self, count: usize, message: Snowflake, theme: &Theme) -> Vec<AnyElement> {
		self.replacement_cards(count)
			.into_iter()
			.enumerate()
			.map(|(index, card)| {
				embed_card(
					card,
					format!("plugin-embed:{}:{index}", message.get()).into(),
					theme,
				)
			})
			.collect()
	}

	fn accessories(&self, view: &MessageView, theme: &Theme) -> Vec<AnyElement> {
		let set = self.set();
		set.patches
			.iter()
			.zip(&set.accessories)
			.filter_map(|(patch, tree)| {
				let tree = tree.as_ref()?;
				let scope = Scope::mount(
					&patch.plugin_id,
					ACCESSORY_SLOT,
					view.message.id.to_string(),
					context_of(view),
				);
				Some(
					div()
						.mt(theme.space_4)
						.child(Ctx::new(tree, scope).column(theme))
						.into_any_element(),
				)
			})
			.collect()
	}

	fn annotations(&self, theme: &Theme, cx: &App) -> Vec<AnyElement> {
		let bridge = cx.global::<PluginBridge>();
		self.set()
			.patches
			.iter()
			.flat_map(|patch| {
				let name = bridge.name_of(&patch.plugin_id, cx);
				patch
					.annotations
					.iter()
					.map(move |annotation| (name.clone(), annotation))
			})
			.map(|(name, annotation)| {
				let color = match annotation.tone {
					Tone::Normal | Tone::Muted => theme.text_muted,
					Tone::Brand => theme.brand,
					Tone::Success => theme.success,
					Tone::Warning => theme.warning,
					Tone::Danger => theme.danger,
				};
				let icon = annotation
					.icon
					.clone()
					.map(|icon| {
						PluginIcon::new(icon)
							.size(rem(CAPTION_ICON))
							.color(color)
							.into_any_element()
					})
					.unwrap_or_else(|| {
						Icon::new(IconKind::Plugins)
							.size(rem(CAPTION_ICON))
							.color(color)
							.into_any_element()
					});
				div()
					.flex()
					.items_center()
					.gap(theme.space_4)
					.mt(rem(2.))
					.text_size(theme.font_tiny)
					.text_color(color)
					.child(icon)
					.child(annotation.text.clone())
					.child(div().text_color(theme.text_muted).child(format!("· {name}")))
					.into_any_element()
			})
			.collect()
	}
}
