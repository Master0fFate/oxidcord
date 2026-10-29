mod activity;
mod custom_status;
mod full_profile;
mod groups;
pub mod pages;
mod parts;
mod picker;
mod profile_data;
pub(crate) mod rich;
mod status;

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
	Context, Entity, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render,
	Styled as _, Subscription, Task, Window, div,
};
use protocol::Snowflake;
use session::Command;

pub use self::activity::{ActivityView, OpenLink, activities, activity_deck};
use self::full_profile::{FullProfile, FullTab};
pub use self::groups::{GroupEvent, Groups};
pub use self::pages::{
	AccountPages, DataDir, Note, PageEvent, Target, activity_page, chat_page, note_page, streamer_page,
};
pub use self::parts::{
	CloseFn, action_row, avatar_ring, badges_row, banner, role_chip, section_label, stat_line,
};
pub use self::picker::{confirm_close, open_create};
pub use self::profile_data::ProfileData;
pub use self::rich::markdown_text;
pub use self::status::{StatusMenu, StatusMenuEvent};
use super::plugins::LinkOrigin;
use super::{SettingsBackend, links};
use crate::data::{Data, DataEvent};
use crate::theme::Theme;

const TICK: Duration = Duration::from_secs(1);

pub struct AccountHub {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	full: Option<FullProfile>,
	requested_icons: HashSet<Snowflake>,
	ticking: bool,
	_subscription: Subscription,
	_ticker: Task<()>,
}

impl AccountHub {
	pub fn new(data: Entity<Data>, backend: Rc<SettingsBackend>, cx: &mut Context<Self>) -> Self {
		let subscription = cx.subscribe(&data, |this, _, event: &DataEvent, cx| {
			if let (DataEvent::Changed(changes), Some(full)) = (event, &this.full)
				&& (changes.loaded.contains(full.user)
					|| changes.relationships
					|| changes.rejected
					|| !changes.presence.is_empty()
					|| this.requested_icons.iter().any(|id| changes.loaded.contains(*id)))
			{
				cx.notify();
			}
		});
		let ticker = cx.spawn(async move |this, cx| {
			loop {
				cx.background_executor().timer(TICK).await;
				let alive = this.update(cx, |this, cx| {
					if this.ticking {
						cx.notify();
					}
				});
				if alive.is_err() {
					return;
				}
			}
		});
		Self {
			data,
			backend,
			full: None,
			requested_icons: HashSet::new(),
			ticking: false,
			_subscription: subscription,
			_ticker: ticker,
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
			self.ticking = false;
			cx.notify();
		}
		was_open
	}

	fn load_activities(&mut self, user: Snowflake, cx: &Context<Self>) -> Vec<ActivityView> {
		let data = self.data.read(cx);
		let activities = activities(&data.state(), user);
		for application_id in activities.iter().filter_map(|activity| activity.missing_icon) {
			if self.requested_icons.insert(application_id) {
				data.send(Command::LoadApplicationIcon { application_id });
			}
		}
		self.ticking = activities.iter().any(ActivityView::is_timed);
		activities
	}
}

impl Render for AccountHub {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let Some(full) = &self.full else {
			return div().into_any_element();
		};
		let user = full.user;
		let theme = Theme::current(cx);
		let profile = {
			let data = self.data.read(cx);
			ProfileData::load(&data.state(), full.user, full.guild)
		};
		let Some(profile) = profile else {
			return div().into_any_element();
		};
		let activities = self.load_activities(user, cx);
		let backend = self.backend.clone();
		let open: OpenLink = Rc::new(move |url, window, cx| {
			links::open_link(url, LinkOrigin::default(), &backend, window, cx);
		});
		let card = self.full_profile_card(&profile, &activities, &open, &theme, cx);
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
