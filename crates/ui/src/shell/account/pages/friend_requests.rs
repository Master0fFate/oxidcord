use gpui::{
	Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window, div,
};
use gpui_component::switch::Switch;
use protocol::{FRIEND_SOURCE_MUTUAL_FRIENDS, FRIEND_SOURCE_MUTUAL_GUILDS, FRIEND_SOURCE_NO_RELATION};
use session::Command;

use super::widgets::{heading, row};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceFlags {
	all: bool,
	mutual_friends: bool,
	mutual_guilds: bool,
}

impl SourceFlags {
	fn from_bits(bits: u32) -> Self {
		Self {
			all: bits & FRIEND_SOURCE_NO_RELATION != 0,
			mutual_friends: bits & FRIEND_SOURCE_MUTUAL_FRIENDS != 0,
			mutual_guilds: bits & FRIEND_SOURCE_MUTUAL_GUILDS != 0,
		}
	}

	fn with_everyone(enabled: bool) -> Self {
		Self {
			all: enabled,
			mutual_friends: enabled,
			mutual_guilds: enabled,
		}
	}

	fn with_friends_of_friends(self, enabled: bool) -> Self {
		Self {
			all: enabled && self.mutual_guilds,
			mutual_friends: enabled,
			..self
		}
	}

	fn with_server_members(self, enabled: bool) -> Self {
		Self {
			all: enabled && self.mutual_friends,
			mutual_guilds: enabled,
			..self
		}
	}

	fn to_bits(self) -> u32 {
		[
			(self.all, FRIEND_SOURCE_NO_RELATION),
			(self.mutual_friends, FRIEND_SOURCE_MUTUAL_FRIENDS),
			(self.mutual_guilds, FRIEND_SOURCE_MUTUAL_GUILDS),
		]
		.into_iter()
		.filter(|(enabled, _)| *enabled)
		.fold(0, |bits, (_, flag)| bits | flag)
	}
}

pub struct FriendRequestsPage {
	data: Entity<Data>,
	_subscription: Subscription,
}

impl FriendRequestsPage {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |_, _, event: &DataEvent, cx| {
			if let DataEvent::Changed(changes) = event
				&& changes.user_settings
			{
				cx.notify();
			}
		});
		Self {
			data,
			_subscription: subscription,
		}
	}

	fn save(&self, flags: SourceFlags, cx: &Context<Self>) {
		self.data
			.read(cx)
			.send(Command::SetFriendSources(flags.to_bits()));
	}
}

impl Render for FriendRequestsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let flags = SourceFlags::from_bits(self.data.read(cx).state().friend_source_flags());
		div()
			.flex()
			.flex_col()
			.child(heading(cx.t("friend-requests-title"), &theme))
			.child(row(
				cx.t("friend-requests-everyone"),
				None,
				Switch::new("friend-requests-everyone")
					.checked(flags.all)
					.on_change(cx.listener(move |this, checked: &bool, _, cx| {
						this.save(SourceFlags::with_everyone(*checked), cx);
					})),
				&theme,
			))
			.child(row(
				cx.t("friend-requests-mutual-friends"),
				None,
				Switch::new("friend-requests-mutual-friends")
					.checked(flags.mutual_friends)
					.on_change(cx.listener(move |this, checked: &bool, _, cx| {
						this.save(flags.with_friends_of_friends(*checked), cx);
					})),
				&theme,
			))
			.child(row(
				cx.t("friend-requests-mutual-guilds"),
				None,
				Switch::new("friend-requests-mutual-guilds")
					.checked(flags.mutual_guilds)
					.on_change(cx.listener(move |this, checked: &bool, _, cx| {
						this.save(flags.with_server_members(*checked), cx);
					})),
				&theme,
			))
	}
}
