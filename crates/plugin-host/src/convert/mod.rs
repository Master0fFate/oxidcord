mod display;
mod md;
mod message;
mod view;

pub use display::{sanitize_badges, sanitize_display_full};
pub use md::{md_to_rich, md_to_tree, rich_to_md, tree_to_md};
pub use message::{
	ExtContext, SanitizedPatch, emoji_ref, empty_patch, empty_patch_ext, message_full, sanitize_patch,
	sanitize_patch_ext,
};
pub use view::{
	COLOR_TOKENS, ImagePolicy, clip, clip_line, default_style, has_key, sanitize_card, sanitize_color,
	sanitize_view, upgrade_ui_tree, valid_icon, valid_key, web_url,
};

use crate::api::{MessageView, UiNode, UiTree, User};

const MAX_UI_NODES: usize = 2_000;
const MAX_UI_STRING: usize = 4_096;

pub fn validate_ui_tree(tree: &UiTree) -> Result<(), &'static str> {
	if tree.nodes.len() > MAX_UI_NODES {
		return Err("ui tree has too many nodes");
	}
	if tree.roots.iter().any(|&root| root as usize >= tree.nodes.len()) {
		return Err("ui root index out of range");
	}
	for (index, node) in tree.nodes.iter().enumerate() {
		validate_ui_node(node, index, tree.nodes.len())?;
	}
	Ok(())
}

fn validate_ui_node(node: &UiNode, index: usize, count: usize) -> Result<(), &'static str> {
	match node {
		UiNode::Column(layout) | UiNode::Row(layout) => {
			let ordered = layout
				.children
				.iter()
				.all(|&child| (child as usize) > index && (child as usize) < count);
			if !ordered {
				return Err("ui child index must point forward and stay in range");
			}
			check_strings(layout.id.iter().chain([&layout.align]))
		}
		UiNode::Text(text) => check_strings([&text.value].into_iter().chain(text.style.color.iter())),
		UiNode::Button(button) => check_strings([&button.id, &button.label, &button.variant]),
		UiNode::Input(input) => check_strings([&input.id, &input.placeholder, &input.value]),
		UiNode::Toggle(toggle) => check_strings([&toggle.id, &toggle.label]),
		UiNode::Image(image) => {
			if !image.url.starts_with("https://") {
				return Err("ui image url must be https");
			}
			check_strings([&image.url])
		}
		UiNode::Badge(badge) => check_strings([&badge.label].into_iter().chain(badge.color.iter())),
		UiNode::Icon(name) => check_strings([name]),
		UiNode::Link(link) => {
			if !(link.url.starts_with("https://") || link.url.starts_with("http://")) {
				return Err("ui link url must be http or https");
			}
			check_strings([&link.label, &link.url])
		}
		UiNode::Divider | UiNode::Progress(_) => Ok(()),
	}
}

fn check_strings<'a>(strings: impl IntoIterator<Item = &'a String>) -> Result<(), &'static str> {
	if strings.into_iter().any(|value| value.len() > MAX_UI_STRING) {
		Err("ui string too long")
	} else {
		Ok(())
	}
}

pub fn user_view(user: &protocol::User) -> User {
	User {
		id: user.id.get(),
		username: user.username.clone(),
		display_name: user
			.global_name
			.clone()
			.filter(|name| !name.is_empty())
			.unwrap_or_else(|| user.username.clone()),
		avatar: user.avatar.clone(),
		bot: user.bot.unwrap_or(false),
	}
}

pub const DELETED_FLAG: u64 = 1 << 63;

pub fn message_view(message: &protocol::Message) -> MessageView {
	MessageView {
		id: message.id.get(),
		channel_id: message.channel_id.get(),
		guild_id: message.guild_id.map(protocol::Snowflake::get),
		author: user_view(&message.author),
		content: message.content.clone(),
		timestamp_ms: message
			.timestamp
			.unwrap_or_else(|| message.id.timestamp())
			.timestamp_millis(),
		edited: message.edited_timestamp.is_some(),
		reply_to: message
			.message_reference
			.as_ref()
			.and_then(|reference| reference.message_id)
			.map(protocol::Snowflake::get),
		flags: message.flags.bits(),
	}
}
