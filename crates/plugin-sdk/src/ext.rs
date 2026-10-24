use crate::md::Md;
use crate::model::{EmojiRef, MessageFull, PickVerdict, UiContext, ViewEvent, ViewReply, ViewTree};
use crate::patch::Patch;
use crate::plugin::Plugin;
use crate::view::{self, Node};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reply {
	replace: Option<ViewTree>,
	close: bool,
	toast: Option<String>,
}

impl Reply {
	pub fn none() -> Self {
		Self::default()
	}

	pub fn replace<N: Into<Node>>(mut self, roots: impl IntoIterator<Item = N>) -> Self {
		self.replace = Some(view::tree(roots));
		self
	}

	pub fn close(mut self) -> Self {
		self.close = true;
		self
	}

	pub fn toast(mut self, text: impl Into<String>) -> Self {
		self.toast = Some(text.into());
		self
	}

	pub(crate) fn into_api(self) -> ViewReply {
		ViewReply {
			replace: self.replace,
			close_overlay: self.close,
			toast: self.toast,
		}
	}
}

pub trait PluginExt: Plugin {
	fn render_view(&mut self, _point: &str, _context: &UiContext) -> Option<Vec<Node>> {
		None
	}

	fn on_view_event(&mut self, _event: ViewEvent) -> Reply {
		Reply::none()
	}

	fn on_menu_item(&mut self, _menu: &str, _key: &str, _context: &UiContext) {}

	fn intercept_message(&mut self, _msg: &MessageFull, _content: Vec<Md>) -> Patch {
		Patch::keep()
	}

	fn on_timer(&mut self, _id: u64, _tag: &str) {}

	fn on_emoji_pick(&mut self, _emoji: &EmojiRef, _channel_id: u64) -> PickVerdict {
		PickVerdict::Default
	}
}

#[doc(hidden)]
pub mod __private {
	use super::*;
	use crate::md;
	use crate::model::{MessagePatch, RichTree};

	pub fn render_view<P: PluginExt>(plugin: &mut P, point: String, context: UiContext) -> Option<ViewTree> {
		plugin.render_view(&point, &context).map(view::tree)
	}

	pub fn on_view_event<P: PluginExt>(plugin: &mut P, event: ViewEvent) -> ViewReply {
		plugin.on_view_event(event).into_api()
	}

	pub fn intercept_message<P: PluginExt>(
		plugin: &mut P,
		msg: MessageFull,
		content: RichTree,
	) -> MessagePatch {
		plugin.intercept_message(&msg, md::from_rich(&content)).into_api()
	}

	pub fn keep() -> MessagePatch {
		Patch::keep().into_api()
	}

	pub fn quiet() -> ViewReply {
		Reply::none().into_api()
	}
}
