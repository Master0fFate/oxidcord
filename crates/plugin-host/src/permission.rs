use serde::{Deserialize, Serialize};

use crate::bindings::oxidcord::plugin::types as api;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
	ContextBasic,
	ContextTimeline,
	ContextDirectory,
	ContextMembers,
	ContextVoice,
	ContextPrivate,
	MessageRender,
	MessageSend,
	MessageFilter,
	UserDisplay,
	ActionsSend,
	ActionsReact,
	ActionsNavigate,
	ActionsClipboard,
	ActionsNotify,
	ActionsOpenUrl,
	Storage,
	NetFetch,
	UiSlots,
}

impl Permission {
	pub const ALL: [Permission; 19] = [
		Self::ContextBasic,
		Self::ContextTimeline,
		Self::ContextDirectory,
		Self::ContextMembers,
		Self::ContextVoice,
		Self::ContextPrivate,
		Self::MessageRender,
		Self::MessageSend,
		Self::MessageFilter,
		Self::UserDisplay,
		Self::ActionsSend,
		Self::ActionsReact,
		Self::ActionsNavigate,
		Self::ActionsClipboard,
		Self::ActionsNotify,
		Self::ActionsOpenUrl,
		Self::Storage,
		Self::NetFetch,
		Self::UiSlots,
	];

	pub fn as_str(self) -> &'static str {
		match self {
			Self::ContextBasic => "context-basic",
			Self::ContextTimeline => "context-timeline",
			Self::ContextDirectory => "context-directory",
			Self::ContextMembers => "context-members",
			Self::ContextVoice => "context-voice",
			Self::ContextPrivate => "context-private",
			Self::MessageRender => "message-render",
			Self::MessageSend => "message-send",
			Self::MessageFilter => "message-filter",
			Self::UserDisplay => "user-display",
			Self::ActionsSend => "actions-send",
			Self::ActionsReact => "actions-react",
			Self::ActionsNavigate => "actions-navigate",
			Self::ActionsClipboard => "actions-clipboard",
			Self::ActionsNotify => "actions-notify",
			Self::ActionsOpenUrl => "actions-open-url",
			Self::Storage => "storage",
			Self::NetFetch => "net-fetch",
			Self::UiSlots => "ui-slots",
		}
	}
}

impl From<Permission> for api::Permission {
	fn from(permission: Permission) -> Self {
		match permission {
			Permission::ContextBasic => Self::ContextBasic,
			Permission::ContextTimeline => Self::ContextTimeline,
			Permission::ContextDirectory => Self::ContextDirectory,
			Permission::ContextMembers => Self::ContextMembers,
			Permission::ContextVoice => Self::ContextVoice,
			Permission::ContextPrivate => Self::ContextPrivate,
			Permission::MessageRender => Self::MessageRender,
			Permission::MessageSend => Self::MessageSend,
			Permission::MessageFilter => Self::MessageFilter,
			Permission::UserDisplay => Self::UserDisplay,
			Permission::ActionsSend => Self::ActionsSend,
			Permission::ActionsReact => Self::ActionsReact,
			Permission::ActionsNavigate => Self::ActionsNavigate,
			Permission::ActionsClipboard => Self::ActionsClipboard,
			Permission::ActionsNotify => Self::ActionsNotify,
			Permission::ActionsOpenUrl => Self::ActionsOpenUrl,
			Permission::Storage => Self::Storage,
			Permission::NetFetch => Self::NetFetch,
			Permission::UiSlots => Self::UiSlots,
		}
	}
}

impl From<api::Permission> for Permission {
	fn from(permission: api::Permission) -> Self {
		match permission {
			api::Permission::ContextBasic => Self::ContextBasic,
			api::Permission::ContextTimeline => Self::ContextTimeline,
			api::Permission::ContextDirectory => Self::ContextDirectory,
			api::Permission::ContextMembers => Self::ContextMembers,
			api::Permission::ContextVoice => Self::ContextVoice,
			api::Permission::ContextPrivate => Self::ContextPrivate,
			api::Permission::MessageRender => Self::MessageRender,
			api::Permission::MessageSend => Self::MessageSend,
			api::Permission::MessageFilter => Self::MessageFilter,
			api::Permission::UserDisplay => Self::UserDisplay,
			api::Permission::ActionsSend => Self::ActionsSend,
			api::Permission::ActionsReact => Self::ActionsReact,
			api::Permission::ActionsNavigate => Self::ActionsNavigate,
			api::Permission::ActionsClipboard => Self::ActionsClipboard,
			api::Permission::ActionsNotify => Self::ActionsNotify,
			api::Permission::ActionsOpenUrl => Self::ActionsOpenUrl,
			api::Permission::Storage => Self::Storage,
			api::Permission::NetFetch => Self::NetFetch,
			api::Permission::UiSlots => Self::UiSlots,
		}
	}
}
