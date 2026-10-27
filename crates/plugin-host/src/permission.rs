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
	UiMount,
	MessageIntercept,
	MessageRetain,
	Composer,
	ContextExpressions,
	ActionsSendUser,
	ChannelsHidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
	Low,
	Medium,
	High,
	Sensitive,
}

impl Permission {
	pub const ALL: [Permission; 26] = [
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
		Self::UiMount,
		Self::MessageIntercept,
		Self::MessageRetain,
		Self::Composer,
		Self::ContextExpressions,
		Self::ActionsSendUser,
		Self::ChannelsHidden,
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
			Self::UiMount => "ui-mount",
			Self::MessageIntercept => "message-intercept",
			Self::MessageRetain => "message-retain",
			Self::Composer => "composer",
			Self::ContextExpressions => "context-expressions",
			Self::ActionsSendUser => "actions-send-user",
			Self::ChannelsHidden => "channels-hidden",
		}
	}

	pub fn tier(self) -> Tier {
		match self {
			Self::ContextBasic
			| Self::ContextExpressions
			| Self::Storage
			| Self::UiSlots
			| Self::UiMount
			| Self::ActionsNavigate
			| Self::ActionsNotify
			| Self::ChannelsHidden => Tier::Low,
			Self::ContextTimeline
			| Self::ContextDirectory
			| Self::ContextMembers
			| Self::ContextVoice
			| Self::MessageRender
			| Self::UserDisplay
			| Self::ActionsReact
			| Self::ActionsClipboard
			| Self::ActionsOpenUrl
			| Self::MessageIntercept
			| Self::Composer => Tier::Medium,
			Self::MessageSend
			| Self::MessageFilter
			| Self::ActionsSend
			| Self::ActionsSendUser
			| Self::NetFetch => Tier::High,
			Self::ContextPrivate | Self::MessageRetain => Tier::Sensitive,
		}
	}

	pub fn denied(self) -> api::HostError {
		match self.api() {
			Some(permission) => api::HostError::Denied(permission),
			None => api::HostError::Invalid(format!("permission {} is not granted", self.as_str())),
		}
	}

	fn api(self) -> Option<api::Permission> {
		Some(match self {
			Self::ContextBasic => api::Permission::ContextBasic,
			Self::ContextTimeline => api::Permission::ContextTimeline,
			Self::ContextDirectory => api::Permission::ContextDirectory,
			Self::ContextMembers => api::Permission::ContextMembers,
			Self::ContextVoice => api::Permission::ContextVoice,
			Self::ContextPrivate => api::Permission::ContextPrivate,
			Self::MessageRender => api::Permission::MessageRender,
			Self::MessageSend => api::Permission::MessageSend,
			Self::MessageFilter => api::Permission::MessageFilter,
			Self::UserDisplay => api::Permission::UserDisplay,
			Self::ActionsSend => api::Permission::ActionsSend,
			Self::ActionsReact => api::Permission::ActionsReact,
			Self::ActionsNavigate => api::Permission::ActionsNavigate,
			Self::ActionsClipboard => api::Permission::ActionsClipboard,
			Self::ActionsNotify => api::Permission::ActionsNotify,
			Self::ActionsOpenUrl => api::Permission::ActionsOpenUrl,
			Self::Storage => api::Permission::Storage,
			Self::NetFetch => api::Permission::NetFetch,
			Self::UiSlots => api::Permission::UiSlots,
			Self::UiMount
			| Self::MessageIntercept
			| Self::MessageRetain
			| Self::Composer
			| Self::ContextExpressions
			| Self::ActionsSendUser
			| Self::ChannelsHidden => return None,
		})
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
