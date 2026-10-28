use crate::md::Md;
use crate::model::{
	Badge, ChannelClick, ChannelDetail, ClientEvent, DisplayOverrideFull, EmojiInfo, EmojiRef, LinkVerdict,
	MessageClick, MessageFull, MessageMeta, MessagePatchExt, NotificationInfo, NotificationVerdict,
	OutgoingFull, PickVerdict, Presence, SendDecisionFull, UiContext, ViewEvent, ViewReply, ViewTree,
};
use crate::patch::{Patch, PatchExt};
use crate::plugin::Plugin;
use crate::types::User;
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

	fn before_send_full(&mut self, _msg: OutgoingFull) -> SendDecisionFull {
		SendDecisionFull::Pass
	}

	fn display_user_full(
		&mut self,
		_guild_id: Option<u64>,
		_user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		None
	}

	fn intercept_message_ext(
		&mut self,
		_msg: &MessageFull,
		_meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		PatchExt::keep()
	}

	fn intercept_messages_ext(&mut self, items: Vec<(MessageFull, MessageMeta, Vec<Md>)>) -> Vec<PatchExt> {
		items
			.into_iter()
			.map(|(msg, meta, content)| self.intercept_message_ext(&msg, &meta, content))
			.collect()
	}

	fn on_client_event(&mut self, _event: ClientEvent) {}

	fn on_message_click(&mut self, _click: MessageClick) {}

	fn on_channel_click(&mut self, _click: ChannelClick) {}

	fn on_link_click(&mut self, _url: &str, _context: &UiContext) -> LinkVerdict {
		LinkVerdict::Pass
	}

	fn on_draft_change(&mut self, _channel_id: u64, _text: &str) {}

	fn autocomplete_emoji(&mut self, _query: &str, _candidates: &[EmojiInfo]) -> Vec<u32> {
		Vec::new()
	}

	fn decorate_channel(&mut self, _channel: &ChannelDetail) -> Vec<Badge> {
		Vec::new()
	}

	fn on_keybind(&mut self, _id: &str) {}

	fn on_notification(&mut self, _info: &NotificationInfo) -> NotificationVerdict {
		NotificationVerdict::Allow
	}

	fn on_badge_click(&mut self, _user_id: u64, _key: &str) {}
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

	pub fn keep_ext() -> MessagePatchExt {
		PatchExt::keep().into_api()
	}

	pub fn intercept_message_ext<P: PluginExt>(
		plugin: &mut P,
		msg: MessageFull,
		meta: MessageMeta,
		content: RichTree,
	) -> MessagePatchExt {
		plugin
			.intercept_message_ext(&msg, &meta, md::from_rich(&content))
			.into_api()
	}

	pub fn intercept_messages_ext<P: PluginExt>(
		plugin: &mut P,
		items: Vec<(MessageFull, MessageMeta, RichTree)>,
	) -> Vec<MessagePatchExt> {
		let items = items
			.into_iter()
			.map(|(msg, meta, content)| (msg, meta, md::from_rich(&content)))
			.collect();
		plugin
			.intercept_messages_ext(items)
			.into_iter()
			.map(PatchExt::into_api)
			.collect()
	}

	pub fn quiet() -> ViewReply {
		Reply::none().into_api()
	}
}
