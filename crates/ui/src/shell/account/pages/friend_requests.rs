use gpui::{
	Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window, div,
};
use gpui_component::switch::Switch;
use serde_json::{Value, json};
use session::Command;

use super::widgets::{heading, row};
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::theme::Theme;

const FLAGS_KEY: &str = "friend_source_flags";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceFlags {
	all: bool,
	mutual_friends: bool,
	mutual_guilds: bool,
}

impl SourceFlags {
	fn read(settings: &Value) -> Self {
		let flag = |name: &str| {
			settings
				.get(FLAGS_KEY)
				.and_then(|flags| flags.get(name)?.as_bool())
		};
		Self {
			all: flag("all").unwrap_or(true),
			mutual_friends: flag("mutual_friends").unwrap_or(true),
			mutual_guilds: flag("mutual_guilds").unwrap_or(true),
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

	fn to_settings(self) -> Value {
		json!({
			FLAGS_KEY: {
				"all": self.all,
				"mutual_friends": self.mutual_friends,
				"mutual_guilds": self.mutual_guilds,
			}
		})
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
			.send(Command::UpdateUserSettings(flags.to_settings()));
	}
}

impl Render for FriendRequestsPage {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let flags = SourceFlags::read(self.data.read(cx).state().user_settings());
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
