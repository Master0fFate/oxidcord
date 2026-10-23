mod custom_status;
mod full_profile;
mod groups;
pub mod pages;
mod parts;
mod picker;
mod profile_data;
mod rich;
mod status;

use gpui::{
	Context, Entity, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render,
	Styled as _, Subscription, Window, div,
};
use protocol::Snowflake;
use session::Command;

use self::full_profile::{FullProfile, FullTab};
pub use self::groups::{GroupEvent, Groups};
pub use self::pages::{AccountPages, DataDir, Note, PageEvent, Target, chat_page, note_page, streamer_page};
pub use self::parts::{
	CloseFn, action_row, avatar_ring, badges_row, banner, role_chip, section_label, stat_line,
};
pub use self::picker::{confirm_close, open_create};
pub use self::profile_data::ProfileData;
pub use self::rich::markdown_text;
pub use self::status::{StatusMenu, StatusMenuEvent};
use crate::data::{Data, DataEvent};
use crate::theme::Theme;

pub struct AccountHub {
	data: Entity<Data>,
	full: Option<FullProfile>,
	_subscription: Subscription,
}

impl AccountHub {
	pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |this, _, event: &DataEvent, cx| {
			if let (DataEvent::Changed(changes), Some(full)) = (event, &this.full)
				&& (changes.loaded.contains(full.user)
					|| changes.relationships
					|| changes.rejected
					|| !changes.presence.is_empty())
			{
				cx.notify();
			}
		});
		Self {
			data,
			full: None,
			_subscription: subscription,
		}
	}

	pub fn open_full_profile(&mut self, user: Snowflake, guild: Option<Snowflake>, cx: &mut Context<Self>) {
		self.data.read(cx).send(Command::LoadProfile {
			user_id: user,
			guild_id: guild,
		});
		self.full = Some(FullProfile {
			user,
			guild,
			tab: FullTab::Info,
		});
		cx.notify();
	}

	pub fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
		let was_open = self.full.take().is_some();
		if was_open {
			cx.notify();
		}
		was_open
	}
}

impl Render for AccountHub {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let Some(full) = &self.full else {
			return div().into_any_element();
		};
		let theme = Theme::current(cx);
		let profile = {
			let data = self.data.read(cx);
			ProfileData::load(&data.state(), full.user, full.guild)
		};
		let Some(profile) = profile else {
			return div().into_any_element();
		};
		let card = self.full_profile_card(&profile, &theme, cx);
		div()
			.id("account-scrim")
			.occlude()
			.absolute()
			.top(theme.title_bar_height)
			.left_0()
			.right_0()
			.bottom_0()
			.flex()
			.items_center()
			.justify_center()
			.p(theme.space_32)
			.bg(theme.overlay)
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(|this, _, _, cx| {
					this.dismiss(cx);
				}),
			)
			.child(card)
			.into_any_element()
	}
}
