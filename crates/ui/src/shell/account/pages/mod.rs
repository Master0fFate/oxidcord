mod accessibility;
mod advanced;
mod chat;
mod devices;
mod friend_requests;
mod language;
mod my_account;
mod notifications;
mod profiles;
mod streamer;
mod widgets;

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{AnyElement, App, Context, Entity, Global, Window};

pub use self::accessibility::AccessibilityPage;
pub use self::advanced::AdvancedPage;
pub use self::chat::chat_page;
pub use self::devices::DevicesPage;
pub use self::friend_requests::FriendRequestsPage;
pub use self::language::LanguagePage;
pub use self::my_account::MyAccountPage;
pub use self::notifications::NotificationsPage;
pub use self::profiles::ProfilesPage;
pub use self::streamer::streamer_page;
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::shell::SettingsBackend;
use crate::theme::Theme;

pub struct DataDir(pub PathBuf);

impl Global for DataDir {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
	Profiles,
}

pub enum PageEvent {
	Goto(Target),
	LogOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
	Privacy,
	Connections,
	Clips,
}

impl Note {
	fn key(self) -> &'static str {
		match self {
			Self::Privacy => "settings-note-privacy",
			Self::Connections => "settings-note-connections",
			Self::Clips => "settings-note-clips",
		}
	}

	fn icon(self) -> IconKind {
		match self {
			Self::Privacy => IconKind::Shield,
			Self::Connections => IconKind::Connections,
			Self::Clips => IconKind::Clips,
		}
	}
}

pub fn note_page(note: Note, cx: &App) -> AnyElement {
	let theme = Theme::current(cx);
	widgets::placeholder(note.icon(), cx.t(note.key()), &theme)
}

pub struct AccountPages {
	pub my_account: Entity<MyAccountPage>,
	pub profiles: Entity<ProfilesPage>,
	pub language: Entity<LanguagePage>,
	pub notifications: Entity<NotificationsPage>,
	pub advanced: Entity<AdvancedPage>,
	pub friend_requests: Entity<FriendRequestsPage>,
	pub devices: Entity<DevicesPage>,
	pub accessibility: Entity<AccessibilityPage>,
}

impl AccountPages {
	pub fn new<T: 'static>(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<T>,
	) -> Self {
		use gpui::AppContext as _;
		Self {
			my_account: cx.new(|cx| MyAccountPage::new(data.clone(), cx)),
			friend_requests: cx.new(|cx| FriendRequestsPage::new(data.clone(), cx)),
			devices: cx.new(|cx| DevicesPage::new(data.clone(), cx)),
			profiles: cx.new(|cx| ProfilesPage::new(data, window, cx)),
			language: cx.new(|_| LanguagePage::new(backend.clone())),
			notifications: cx.new(|_| NotificationsPage::new(backend.clone())),
			accessibility: cx.new(|cx| AccessibilityPage::new(backend.clone(), cx)),
			advanced: cx.new(|_| AdvancedPage::new(backend)),
		}
	}
}
