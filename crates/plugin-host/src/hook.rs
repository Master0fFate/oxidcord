use serde::{Deserialize, Serialize};

use crate::permission::Permission;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Hook {
	DraftChange,
	AutocompleteEmoji,
	DecorateChannel,
	Notification,
	MessageClick,
	ChannelClick,
	LinkClick,
	DisplayUser,
	BeforeSend,
	Intercept,
}

impl Hook {
	pub const ALL: [Hook; 10] = [
		Self::DraftChange,
		Self::AutocompleteEmoji,
		Self::DecorateChannel,
		Self::Notification,
		Self::MessageClick,
		Self::ChannelClick,
		Self::LinkClick,
		Self::DisplayUser,
		Self::BeforeSend,
		Self::Intercept,
	];

	pub fn as_str(self) -> &'static str {
		match self {
			Self::DraftChange => "draft-change",
			Self::AutocompleteEmoji => "autocomplete-emoji",
			Self::DecorateChannel => "decorate-channel",
			Self::Notification => "notification",
			Self::MessageClick => "message-click",
			Self::ChannelClick => "channel-click",
			Self::LinkClick => "link-click",
			Self::DisplayUser => "display-user",
			Self::BeforeSend => "before-send",
			Self::Intercept => "intercept",
		}
	}

	pub fn permission(self) -> Permission {
		match self {
			Self::DraftChange => Permission::ComposerWatch,
			Self::AutocompleteEmoji | Self::DecorateChannel | Self::MessageClick | Self::ChannelClick => {
				Permission::UiInput
			}
			Self::Notification => Permission::NotificationsFilter,
			Self::LinkClick => Permission::ActionsOpenUrl,
			Self::DisplayUser => Permission::UserDisplay,
			Self::BeforeSend => Permission::MessageSend,
			Self::Intercept => Permission::MessageIntercept,
		}
	}
}
