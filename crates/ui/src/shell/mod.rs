mod account;
mod attachments;
mod channel_sidebar;
mod community;
mod composer;
mod dm_sidebar;
mod emoji_picker;
mod friends;
mod history;
mod inline_emoji;
mod lightbox;
mod links;
mod login;
mod member_list;
mod message;
mod moderation;
mod pickers;
mod plugins;
mod profile;
mod quick_switcher;
mod search_hints;
mod search_panel;
mod server_rail;
mod server_settings;
mod settings;
mod threads;
mod timeline;
mod title_bar;
mod top_bar;
mod transition;
mod updates;
mod user_panel;
mod voice;

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
	Anchor, AnyElement, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
	InteractiveElement as _, IntoElement, MouseButton, NavigationDirection, ParentElement as _, Pixels,
	Point, Render, SharedString, Styled as _, Subscription, Task, Window, anchored, deferred, div,
	prelude::FluentBuilder as _,
};
use gpui_component::WindowExt as _;
use platform::notifications::Notification;
use protocol::{ChannelType, Snowflake, StatusKind};
use session::Command;
use state::{Alert, Changes, State};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use self::account::{AccountHub, GroupEvent, Groups};
use self::channel_sidebar::{ChannelSidebar, SidebarEvent};
use self::community::{Community, CommunityEvent, ComposerSlot, PickerTarget};
use self::composer::{Composer, ComposerEvent};
use self::dm_sidebar::{DmSidebar, DmSidebarEvent};
use self::friends::{FriendsEvent, HomePage, HomePages};
use self::history::{History, Place};
use self::member_list::{MemberList, MemberListEvent};
use self::message::{ImageTarget, snippet};
use self::pickers::PickerTab;
use self::plugins::{DebugPanel, PluginUi, PluginUiEvent, RightPanel as PluginRightPanel, SidebarPanel};
use self::profile::{ProfileCard, ProfileEvent};
use self::quick_switcher::{QuickSwitcher, QuickSwitcherEvent};
use self::search_panel::{SearchEvent, SearchPanel};
use self::server_rail::{RailEvent, ServerRail};
use self::settings::{SettingsEvent, SettingsWindow};
use self::timeline::{Timeline, TimelineEvent};
use self::title_bar::{TitleBar, TitleContext};
use self::top_bar::{TopBar, TopBarEvent, TopBarTitle};
use self::user_panel::{UserPanel, UserPanelEvent};
use self::voice::{CallToast, RoomEvent, UserMenu, UserMenuEvent, VoicePanel, VoiceRoom};
use crate::actions::{
	CloseOverlay, NextChannel, OpenQuickSwitcher, OpenSettings, PreviousChannel, SHELL_CONTEXT,
};
use crate::components::IconButton;
use crate::data::{Data, DataEvent};
use crate::i18n::Translate as _;
use crate::icons::{IconKind, channel_icon};
use crate::model::{Status, status_of};
use crate::theme::{Theme, rem};

pub use self::account::DataDir;
pub use self::login::{LoginEvent, LoginOptions, LoginScreen};
pub use self::plugins::PluginStore;
pub use self::quick_switcher::{
	Category as SwitcherCategory, Leading as SwitcherLeading, SwitcherEntry, SwitcherTarget,
};
pub use self::settings::SettingsBackend;
pub use self::transition::set_reduced_motion;
pub use self::updates::UpdateService;
pub use self::voice::{Textures, VideoTextures, VoiceHotkeys};

const MEMBER_LIST_MIN_WIDTH: f32 = 1200.;
const SIDEBAR_MIN_WIDTH: f32 = 900.;
const WINDOW_TITLE: &str = "Oxidcord";
const NOTIFICATION_BODY_LIMIT: usize = 140;
const RECENT_TARGETS: usize = 8;

pub enum ShellEvent {
	LoggedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightPanel {
	Members,
	Search,
}

enum Overlay {
	Lightbox(ImageTarget),
	Profile {
		card: Entity<ProfileCard>,
		at: Point<Pixels>,
		anchor: Anchor,
	},
	VoiceUser {
		menu: Entity<UserMenu>,
		at: Point<Pixels>,
	},
}

struct Described {
	name: SharedString,
	icon: IconKind,
	topic: Option<SharedString>,
	direct: bool,
	status: Status,
}

pub struct Shell {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	focus: FocusHandle,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
	home: bool,
	initial_selection_sent: bool,
	last_channel: HashMap<Snowflake, Snowflake>,
	last_dm: Option<Snowflake>,
	recent_targets: Vec<SwitcherTarget>,
	history: History,
	panel: RightPanel,
	members_visible: bool,
	pending_jump: Option<Snowflake>,
	seen_alert: u64,
	notification_clicks: UnboundedSender<Snowflake>,
	title_mentions: u32,
	rail: Entity<ServerRail>,
	channel_sidebar: Entity<ChannelSidebar>,
	dm_sidebar: Entity<DmSidebar>,
	user_panel: Entity<UserPanel>,
	voice_panel: Entity<VoicePanel>,
	voice_room: Entity<VoiceRoom>,
	call_toast: Entity<CallToast>,
	home_pages: HomePages,
	account: Entity<AccountHub>,
	groups: Groups,
	top_bar: Entity<TopBar>,
	timeline: Entity<Timeline>,
	composer: Entity<Composer>,
	community: Entity<Community>,
	member_list: Entity<MemberList>,
	search_panel: Entity<SearchPanel>,
	plugin_ui: Entity<PluginUi>,
	settings: Option<Entity<SettingsWindow>>,
	overlay: Option<Overlay>,
	_subscriptions: Vec<Subscription>,
	_notification_clicks: Task<()>,
}

impl EventEmitter<ShellEvent> for Shell {}

impl Focusable for Shell {
	fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
		self.focus.clone()
	}
}

fn default_channel(state: &State, guild: Snowflake) -> Option<Snowflake> {
	state
		.guild_channel_tree(guild)
		.iter()
		.flat_map(|node| node.children.iter())
		.find(|channel| {
			!channel.locked
				&& matches!(
					channel.kind,
					ChannelType::GuildText | ChannelType::GuildAnnouncement
				)
		})
		.map(|channel| channel.id)
}

fn selectable_channels(state: &State, guild: Snowflake) -> Vec<Snowflake> {
	state
		.guild_channel_tree(guild)
		.iter()
		.flat_map(|node| node.children.iter())
		.filter(|channel| {
			!channel.locked && !matches!(channel.kind, ChannelType::GuildForum | ChannelType::GuildMedia)
		})
		.map(|channel| channel.id)
		.collect()
}

fn alert_notification(state: &State, alert: &Alert, show_content: bool) -> Notification {
	let message = &alert.message;
	let author = state.display_name(alert.guild_id, message.author.id);
	let channel = state.channel(message.channel_id);
	let title = match (alert.guild_id.and_then(|id| state.guild(id)), channel) {
		(Some(guild), Some(channel)) => format!(
			"{author} ({} / #{})",
			guild.name,
			channel.name.as_deref().unwrap_or_default()
		),
		(Some(guild), None) => format!("{author} ({})", guild.name),
		(None, Some(channel)) if channel.kind == ChannelType::GroupDm => match channel.name.as_deref() {
			Some(name) if !name.is_empty() => format!("{author} ({name})"),
			_ => author,
		},
		_ => author,
	};
	let body = if show_content {
		snippet(&markdown::parse(&message.content), NOTIFICATION_BODY_LIMIT)
	} else {
		String::new()
	};
	Notification {
		title,
		body,
		icon_path: None,
		app_id: crate::APP_ID.to_owned(),
	}
}

fn describe(state: &State, guild: Option<Snowflake>, channel: Snowflake) -> Described {
	match guild {
		Some(_) => {
			let found = state.channel(channel);
			Described {
				name: found
					.and_then(|channel| channel.name.clone())
					.unwrap_or_default()
					.into(),
				icon: found.map_or(IconKind::Hash, |channel| channel_icon(channel.kind)),
				topic: found
					.and_then(|channel| channel.topic.clone())
					.filter(|topic| !topic.is_empty())
					.map(Into::into),
				direct: false,
				status: Status::Offline,
			}
		}
		None => {
			let row = state.dm_rows().into_iter().find(|row| row.channel_id == channel);
			let single = row
				.as_ref()
				.filter(|row| row.recipients.len() == 1)
				.map(|row| row.recipients[0]);
			Described {
				name: row.map(|row| row.name).unwrap_or_default().into(),
				icon: IconKind::AtSign,
				topic: None,
				direct: true,
				status: single.map_or(Status::Offline, |user| status_of(state, user)),
			}
		}
	}
}

impl Shell {
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let compact = (backend.read)().appearance.compact_messages;
		let mut subscriptions = Vec::new();
		let plugin_ui = plugins::install(data.clone(), backend.clone(), cx);
		subscriptions.push(cx.observe(&plugin_ui, |_, _, cx| cx.notify()));
		updates::UpdatesPage::watch(cx);

		let rail = cx.new(|cx| ServerRail::new(data.clone(), cx));
		subscriptions.push(cx.subscribe_in(
			&plugin_ui,
			window,
			|this, _, event: &PluginUiEvent, window, cx| match event {
				PluginUiEvent::Insert(text) => {
					let composer = this.community.read(cx).plugin_composer(cx);
					composer.update(cx, |composer, cx| composer.insert(text, window, cx));
				}
				PluginUiEvent::PageChanged => this.rail.update(cx, |_, cx| cx.notify()),
			},
		));
		subscriptions.push(cx.subscribe_in(
			&rail,
			window,
			|this, _, event: &RailEvent, window, cx| match event {
				RailEvent::SelectHome => this.show_home(window, cx),
				RailEvent::SelectGuild(id) => this.show_guild(*id, window, cx),
			},
		));

		let channel_sidebar = cx.new(|_| ChannelSidebar::new(data.clone()));
		subscriptions.push(cx.subscribe_in(
			&channel_sidebar,
			window,
			|this, _, event: &SidebarEvent, _, cx| match event {
				SidebarEvent::SelectChannel(id) => this.select(this.guild, *id, cx),
				SidebarEvent::VoiceUser { user, at } => this.open_voice_menu(*user, *at, cx),
			},
		));

		let dm_sidebar = cx.new(|cx| DmSidebar::new(data.clone(), cx));
		subscriptions.push(cx.subscribe_in(
			&dm_sidebar,
			window,
			|this, _, event: &DmSidebarEvent, window, cx| match event {
				DmSidebarEvent::OpenDm(id) => this.select(None, *id, cx),
				DmSidebarEvent::Search => this.open_quick_switcher(window, cx),
				DmSidebarEvent::OpenFriends => this.home_pages.show(HomePage::Friends, cx),
				DmSidebarEvent::OpenNitro => this.home_pages.show(HomePage::Nitro, cx),
				DmSidebarEvent::OpenShop => this.home_pages.show(HomePage::Shop, cx),
				DmSidebarEvent::CreateGroup => account::open_create(this.data.clone(), window, cx),
				DmSidebarEvent::CloseDm(id) => this.confirm_close_dm(*id, window, cx),
			},
		));

		let user_panel = cx.new(|cx| UserPanel::new(data.clone(), cx));
		subscriptions.push(cx.subscribe_in(
			&user_panel,
			window,
			|this, _, event: &UserPanelEvent, window, cx| match event {
				UserPanelEvent::OpenSettings => this.open_settings(window, cx),
				UserPanelEvent::EditProfile => {
					this.open_settings(window, cx);
					if let Some(settings) = &this.settings {
						settings.update(cx, |settings, cx| settings.show_profiles(window, cx));
					}
				}
				UserPanelEvent::LogOut => this.log_out(window, cx),
			},
		));

		let top_bar = cx.new(|cx| {
			TopBar::new(
				TopBarTitle::Direct {
					name: SharedString::default(),
					status: Status::Offline,
				},
				window,
				cx,
			)
		});
		subscriptions.push(
			cx.subscribe_in(
				&top_bar,
				window,
				|this, _, event: &TopBarEvent, window, cx| match event {
					TopBarEvent::ToggleMembers => {
						this.plugin_ui.update(cx, |ui, cx| ui.close_right(cx));
						this.members_visible = !this.members_visible;
						this.panel = RightPanel::Members;
						let visible = this.members_visible;
						this.top_bar
							.update(cx, |bar, cx| bar.set_members_visible(visible, cx));
						cx.notify();
					}
					TopBarEvent::Search(query) => this.run_search(query.clone(), cx),
					TopBarEvent::Call { video } => this.start_call(*video, cx),
					TopBarEvent::Threads { at } => this
						.community
						.update(cx, |community, cx| community.open_threads(*at, window, cx)),
					TopBarEvent::Pins { at } => this
						.community
						.update(cx, |community, cx| community.open_pins(*at, window, cx)),
				},
			),
		);

		let timeline = cx.new(|cx| Timeline::new(data.clone(), compact, cx));
		subscriptions.push(cx.subscribe_in(
			&timeline,
			window,
			|this, _, event: &TimelineEvent, window, cx| this.on_timeline(event, window, cx),
		));

		let composer = cx.new(|cx| Composer::new(data.clone(), window, cx));
		subscriptions.push(cx.subscribe_in(
			&composer,
			window,
			|this, _, event: &ComposerEvent, window, cx| {
				match event {
					ComposerEvent::OpenPicker { tab, at } => this.community.update(cx, |community, cx| {
						community.open_picker(
							*tab,
							PickerTarget::Composer(ComposerSlot::Main),
							*at,
							window,
							cx,
						)
					}),
					ComposerEvent::Sent => this
						.timeline
						.update(cx, |timeline, cx| timeline.scroll_to_end(cx)),
				}
			},
		));

		let community =
			cx.new(|cx| Community::new(data.clone(), backend.clone(), composer.clone(), window, cx));
		Community::install(&community, data.clone(), cx);
		subscriptions.push(cx.subscribe_in(
			&community,
			window,
			|this, _, event: &CommunityEvent, window, cx| {
				match event {
					CommunityEvent::OpenImage(target) => {
						this.overlay = Some(Overlay::Lightbox(target.clone()));
						cx.notify();
					}
					CommunityEvent::OpenProfile { user, at } => {
						this.open_profile(*user, *at, Anchor::TopLeft, cx)
					}
					CommunityEvent::Jump { channel, message } => this.jump_to(*channel, *message, cx),
					CommunityEvent::SearchIn(channel) => this
						.top_bar
						.update(cx, |bar, cx| bar.search_in(*channel, window, cx)),
					CommunityEvent::Layout => cx.notify(),
				}
			},
		));

		let voice_panel = cx.new(|cx| VoicePanel::new(data.clone(), backend.clone(), window, cx));
		let voice_room = cx.new(|cx| {
			VoiceRoom::new(
				data.clone(),
				backend.clone(),
				timeline.clone().into(),
				composer.clone().into(),
				window,
				cx,
			)
		});
		subscriptions.push(
			cx.subscribe_in(
				&voice_room,
				window,
				|this, _, event: &RoomEvent, window, cx| match event {
					RoomEvent::UserMenu { user, at } => this.open_voice_menu(*user, *at, cx),
					RoomEvent::OpenSettings => {
						this.open_settings(window, cx);
						if let Some(settings) = &this.settings {
							settings.update(cx, |settings, cx| settings.show_voice(window, cx));
						}
					}
				},
			),
		);
		let call_toast = cx.new(|cx| CallToast::new(data.clone(), window, cx));
		let home_pages = HomePages::new(data.clone(), window, cx);
		let account = cx.new(|cx| AccountHub::new(data.clone(), cx));
		let groups = Groups::new(data.clone(), window, cx);
		subscriptions.push(cx.subscribe(&groups.members, |this, _, event: &GroupEvent, cx| {
			let GroupEvent::OpenProfile { user, at } = event;
			this.open_profile(*user, *at, Anchor::TopLeft, cx);
		}));
		subscriptions.push(cx.subscribe_in(
			&home_pages.friends,
			window,
			|this, _, event: &FriendsEvent, _, cx| this.on_friends(event, cx),
		));
		let push_to_talk = (backend.read)().voice.push_to_talk;
		data.update(cx, |data, _| data.restore_input_mode(push_to_talk));

		let member_list = cx.new(|_| MemberList::new(data.clone()));
		subscriptions.push(cx.subscribe_in(
			&member_list,
			window,
			|this, _, event: &MemberListEvent, _, cx| {
				let MemberListEvent::OpenProfile { user, at } = event;
				this.open_profile(*user, *at, Anchor::TopRight, cx);
			},
		));

		let search_panel = cx.new(|_| SearchPanel::new(data.clone()));
		subscriptions.push(
			cx.subscribe_in(
				&search_panel,
				window,
				|this, _, event: &SearchEvent, _, cx| match event {
					SearchEvent::Jump { channel, message } => this.jump_to(*channel, *message, cx),
					SearchEvent::Close => {
						this.panel = RightPanel::Members;
						cx.notify();
					}
				},
			),
		);

		subscriptions.push(cx.subscribe_in(
			&data,
			window,
			|this, _, event: &DataEvent, window, cx| match event {
				DataEvent::Changed(changes) => this.on_changes(changes, window, cx),
				DataEvent::MediaReady => this.on_media_ready(cx),
			},
		));

		let focus = cx.focus_handle();
		window.focus(&focus, cx);

		let (notification_clicks, mut clicked) = unbounded_channel();
		let notification_task = cx.spawn_in(window, async move |this, cx| {
			while let Some(channel) = clicked.recv().await {
				let opened = this.update_in(cx, |shell, window, cx| shell.open_notified(channel, window, cx));
				if opened.is_err() {
					break;
				}
			}
		});
		let seen_alert = data.read(cx).state().last_alert_id();

		let mut shell = Self {
			data,
			backend,
			focus,
			guild: None,
			channel: None,
			home: false,
			initial_selection_sent: false,
			last_channel: HashMap::new(),
			last_dm: None,
			recent_targets: Vec::new(),
			history: History::default(),
			panel: RightPanel::Members,
			members_visible: true,
			pending_jump: None,
			seen_alert,
			notification_clicks,
			title_mentions: 0,
			rail,
			channel_sidebar,
			dm_sidebar,
			user_panel,
			voice_panel,
			voice_room,
			call_toast,
			home_pages,
			account,
			groups,
			top_bar,
			timeline,
			composer,
			community,
			member_list,
			search_panel,
			plugin_ui,
			settings: None,
			overlay: None,
			_subscriptions: subscriptions,
			_notification_clicks: notification_task,
		};
		shell.on_changes(&Changes::everything(), window, cx);
		shell
	}

	fn send(&self, command: Command, cx: &gpui::App) {
		self.data.read(cx).send(command);
	}

	fn select(&mut self, guild: Option<Snowflake>, channel: Snowflake, cx: &mut Context<Self>) {
		self.home_pages.close(cx);
		self.plugin_ui.update(cx, |ui, cx| ui.close_page(cx));
		self.send(
			Command::SelectChannel {
				guild_id: guild,
				channel_id: channel,
			},
			cx,
		);
	}

	fn sync_group_header(&mut self, cx: &mut Context<Self>) {
		let group = self.groups.active(cx).then(|| self.groups.bar.clone().into());
		self.top_bar.update(cx, |bar, cx| bar.set_group(group, cx));
	}

	fn refresh_direct_title(&mut self, cx: &mut Context<Self>) {
		let (None, Some(channel)) = (self.guild, self.channel) else {
			return;
		};
		let item = describe(&self.data.read(cx).state(), None, channel);
		let title = TopBarTitle::Direct {
			name: item.name,
			status: item.status,
		};
		self.top_bar.update(cx, |bar, cx| bar.set_title(title, cx));
	}

	fn remember_target(&mut self, target: SwitcherTarget) {
		self.recent_targets.retain(|recent| *recent != target);
		self.recent_targets.insert(0, target);
		self.recent_targets.truncate(RECENT_TARGETS);
	}

	fn log_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.send(Command::Logout, cx);
		self.close_settings(window, cx);
		cx.emit(ShellEvent::LoggedOut);
	}

	fn confirm_close_dm(&mut self, channel: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let group = self
			.data
			.read(cx)
			.state()
			.channel(channel)
			.is_some_and(|found| found.kind == ChannelType::GroupDm);
		account::confirm_close(self.data.clone(), channel, group, window, cx);
	}

	fn on_friends(&mut self, event: &FriendsEvent, cx: &mut Context<Self>) {
		match event {
			FriendsEvent::OpenDm => self.home_pages.close(cx),
			FriendsEvent::OpenProfile { user, at } => self.open_profile(*user, *at, Anchor::TopLeft, cx),
		}
	}

	fn show_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.plugin_ui.update(cx, |ui, cx| ui.close_page(cx));
		self.home = true;
		self.guild = None;
		let dm = {
			let data = self.data.read(cx);
			let state = data.state();
			self.last_dm
				.filter(|dm| state.channel(*dm).is_some())
				.or_else(|| state.dm_rows().first().map(|row| row.channel_id))
		};
		match dm {
			Some(dm) => self.select(None, dm, cx),
			None => {
				self.channel = None;
				self.apply_selection(window, cx);
			}
		}
		self.rail.update(cx, |rail, cx| rail.select(None, cx));
		cx.notify();
	}

	fn show_guild(&mut self, guild: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		self.plugin_ui.update(cx, |ui, cx| ui.close_page(cx));
		let target = {
			let data = self.data.read(cx);
			let state = data.state();
			let remembered = self
				.last_channel
				.get(&guild)
				.copied()
				.filter(|channel| selectable_channels(&state, guild).contains(channel));
			remembered.or_else(|| default_channel(&state, guild))
		};
		match target {
			Some(channel) => self.select(Some(guild), channel, cx),
			None => {
				self.home = false;
				self.guild = Some(guild);
				self.channel = None;
				self.apply_selection(window, cx);
			}
		}
	}

	fn step_channel(&mut self, forward: bool, cx: &mut Context<Self>) {
		let (Some(guild), Some(current)) = (self.guild, self.channel) else {
			return;
		};
		let channels = selectable_channels(&self.data.read(cx).state(), guild);
		let Some(index) = channels.iter().position(|channel| *channel == current) else {
			return;
		};
		let next = if forward {
			(index + 1) % channels.len()
		} else {
			(index + channels.len() - 1) % channels.len()
		};
		self.select(Some(guild), channels[next], cx);
	}

	fn ensure_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.initial_selection_sent || self.data.read(cx).state().selected().is_some() {
			return;
		}
		self.initial_selection_sent = true;
		self.show_friends(window, cx);
	}

	fn show_friends(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.plugin_ui.update(cx, |ui, cx| ui.close_page(cx));
		self.home = true;
		self.guild = None;
		self.channel = None;
		self.apply_selection(window, cx);
		self.home_pages.show(HomePage::Friends, cx);
		cx.notify();
	}

	fn sync_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let selected = self.data.read(cx).state().selected();
		let Some(selected) = selected else {
			if self.channel.is_some() {
				self.show_friends(window, cx);
			}
			return;
		};
		let selected = self
			.community
			.update(cx, |community, cx| community.route(selected, window, cx));
		if self.channel == Some(selected.channel_id) && self.guild == selected.guild_id {
			return;
		}
		self.guild = selected.guild_id;
		self.channel = Some(selected.channel_id);
		self.home = selected.guild_id.is_none();
		match selected.guild_id {
			Some(guild) => {
				self.last_channel.insert(guild, selected.channel_id);
			}
			None => self.last_dm = Some(selected.channel_id),
		}
		self.remember_target(SwitcherTarget::Channel {
			guild: selected.guild_id,
			channel: selected.channel_id,
		});
		self.history.visit(Place {
			guild: selected.guild_id,
			channel: selected.channel_id,
		});
		self.home_pages.close(cx);
		self.apply_selection(window, cx);
	}

	fn apply_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let guild = self.guild;
		let channel = self.channel;
		let described = channel.map(|channel| describe(&self.data.read(cx).state(), guild, channel));
		let (name, direct) = described
			.as_ref()
			.map_or((SharedString::default(), self.home), |item| {
				(item.name.clone(), item.direct)
			});
		self.rail.update(cx, |rail, cx| rail.select(guild, cx));
		self.channel_sidebar
			.update(cx, |sidebar, cx| sidebar.show(guild, channel, cx));
		self.dm_sidebar.update(cx, |sidebar, cx| {
			sidebar.select(channel.filter(|_| guild.is_none()), cx)
		});
		self.member_list.update(cx, |list, cx| list.show(guild, cx));
		self.groups.show(channel.filter(|_| guild.is_none()), cx);
		self.sync_group_header(cx);
		let title = match described {
			Some(item) if item.direct => TopBarTitle::Direct {
				name: item.name,
				status: item.status,
			},
			Some(item) => TopBarTitle::Channel {
				name: item.name,
				icon: item.icon,
				topic: item.topic,
			},
			None => TopBarTitle::Channel {
				name: SharedString::default(),
				icon: IconKind::Hash,
				topic: None,
			},
		};
		let context = plugin_host::ViewContext {
			guild_id: guild.map(Snowflake::get),
			channel_id: channel.map(Snowflake::get),
			..plugin_host::ViewContext::default()
		};
		self.top_bar.update(cx, |bar, cx| {
			bar.set_title(title, cx);
			bar.set_plugin_context(context, cx);
		});
		self.community
			.update(cx, |community, cx| community.show_main(guild, channel, cx));
		self.timeline.update(cx, |timeline, cx| {
			timeline.show(guild, channel, name.clone(), direct, window, cx)
		});
		self.composer.update(cx, |composer, cx| {
			composer.show(guild, channel, name, direct, window, cx)
		});
		if let Some(message) = self.pending_jump.take() {
			self.timeline
				.update(cx, |timeline, cx| timeline.jump_to(message, cx));
		}
		self.set_typing(cx);
		cx.notify();
	}

	fn set_typing(&mut self, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let names: Vec<SharedString> = {
			let data = self.data.read(cx);
			let state = data.state();
			state
				.typing_users(channel, Instant::now())
				.into_iter()
				.map(|user| state.display_name(self.guild, user).into())
				.collect()
		};
		self.composer
			.update(cx, |composer, cx| composer.set_typing(names, cx));
	}

	fn on_changes(&mut self, changes: &Changes, window: &mut Window, cx: &mut Context<Self>) {
		if changes.guild_list || changes.private_channels {
			self.ensure_selection(window, cx);
		}
		if changes.selection {
			self.sync_selection(window, cx);
		}
		if changes.guild_list || changes.read_state {
			self.rail.update(cx, |rail, cx| rail.refresh(cx));
		}
		self.home_pages.on_changes(changes, cx);
		if changes.private_channels || changes.relationships || changes.presence_any() {
			self.groups.refresh(cx);
			self.sync_group_header(cx);
			self.refresh_direct_title(cx);
		}
		if changes.private_channels || changes.read_state || changes.presence_any() || changes.relationships {
			self.dm_sidebar.update(cx, |sidebar, cx| sidebar.refresh(cx));
		}
		if let Some(guild) = self.guild
			&& (changes.channel_lists.contains(guild)
				|| changes.read_state
				|| changes.voice.contains(guild)
				|| changes.voice_session
				|| changes.streams
				|| changes.guild_list)
		{
			self.channel_sidebar.update(cx, |sidebar, cx| sidebar.refresh(cx));
		}
		if changes.current_user || changes.own_status || changes.session || changes.presence_any() {
			self.user_panel.update(cx, |panel, cx| panel.refresh(cx));
		}
		if changes.session {
			let connected = self.data.read(cx).state().session().connected;
			self.timeline
				.update(cx, |timeline, cx| timeline.set_connected(connected, cx));
		}
		if let Some(guild) = self.guild
			&& (changes.members.contains(guild) || changes.presence_any() || changes.guild_list)
		{
			self.member_list.update(cx, |list, cx| list.refresh(cx));
		}
		self.timeline
			.update(cx, |timeline, cx| timeline.on_changes(changes, window, cx));
		self.community
			.update(cx, |community, cx| community.on_changes(changes, window, cx));
		if self
			.channel
			.is_some_and(|channel| changes.typing.contains(channel))
		{
			self.set_typing(cx);
		}
		if self
			.channel
			.is_some_and(|channel| changes.drafts.contains(channel))
		{
			self.composer
				.update(cx, |composer, cx| composer.on_draft(window, cx));
		}
		if changes.search {
			self.search_panel.update(cx, |panel, cx| panel.refresh(cx));
		}
		if changes.plugin_slots {
			self.rail.update(cx, |_, cx| cx.notify());
			self.timeline
				.update(cx, |timeline, cx| timeline.on_plugin_slots(cx));
			self.composer.update(cx, |_, cx| cx.notify());
			self.channel_sidebar.update(cx, |sidebar, cx| sidebar.refresh(cx));
			self.member_list.update(cx, |_, cx| cx.notify());
			self.top_bar.update(cx, |_, cx| cx.notify());
			cx.notify();
		}
		if changes.read_state || changes.guild_list || changes.private_channels {
			self.update_mentions(window, cx);
		}
		if changes.alerts {
			self.raise_alerts(window, cx);
		}
	}

	fn on_media_ready(&mut self, cx: &mut Context<Self>) {
		self.rail.update(cx, |_, cx| cx.notify());
		self.channel_sidebar.update(cx, |_, cx| cx.notify());
		self.dm_sidebar.update(cx, |_, cx| cx.notify());
		self.user_panel.update(cx, |_, cx| cx.notify());
		self.member_list.update(cx, |_, cx| cx.notify());
		self.timeline.update(cx, |_, cx| cx.notify());
		self.composer.update(cx, |_, cx| cx.notify());
		self.search_panel.update(cx, |_, cx| cx.notify());
		self.community
			.update(cx, |community, cx| community.on_media_ready(cx));
		cx.notify();
	}

	fn update_mentions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let total = {
			let data = self.data.read(cx);
			let state = data.state();
			state
				.guild_summaries()
				.iter()
				.map(|summary| summary.mentions)
				.sum::<u32>()
				+ state.dm_rows().iter().map(|row| row.mentions).sum::<u32>()
		};
		if total != self.title_mentions {
			self.title_mentions = total;
			let title = if total > 0 {
				format!("({total}) {WINDOW_TITLE}")
			} else {
				WINDOW_TITLE.to_owned()
			};
			window.set_window_title(&title);
		}
	}

	fn raise_alerts(&mut self, window: &Window, cx: &mut Context<Self>) {
		let settings = (self.backend.read)().notifications;
		let viewing = self.channel.filter(|_| window.is_window_active());
		let notifications: Vec<(Snowflake, Notification)> = {
			let data = self.data.read(cx);
			let state = data.state();
			let seen = std::mem::replace(&mut self.seen_alert, state.last_alert_id());
			if state.own_status() == StatusKind::Dnd || !(settings.enabled || settings.sounds) {
				return;
			}
			let mut alerts = state
				.alerts_after(seen)
				.filter(|alert| Some(alert.message.channel_id) != viewing)
				.peekable();
			if alerts.peek().is_none() {
				return;
			}
			if settings.enabled {
				alerts
					.map(|alert| {
						(
							alert.message.channel_id,
							alert_notification(&state, alert, settings.show_content),
						)
					})
					.collect()
			} else {
				Vec::new()
			}
		};
		if settings.sounds && !crate::data::streamer_mode(cx).mutes_sounds() {
			platform::sound::play_chime();
		}
		for (channel, notification) in notifications {
			let clicks = self.notification_clicks.clone();
			platform::notifications::show_with_click(notification, move || {
				clicks.send(channel).ok();
			});
		}
	}

	fn open_notified(&mut self, channel: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		crate::desktop::show_window(window);
		let guild = self.data.read(cx).state().channel_guild(channel);
		self.select(guild, channel, cx);
	}

	fn run_search(&mut self, query: String, cx: &mut Context<Self>) {
		self.plugin_ui.update(cx, |ui, cx| ui.close_right(cx));
		self.panel = RightPanel::Search;
		let (guild, channel) = (self.guild, self.channel);
		let channel_id = if guild.is_none() { channel } else { None };
		let query = self
			.search_panel
			.update(cx, |panel, cx| panel.begin(&query, guild, channel, cx));
		self.send(
			Command::Search {
				guild_id: guild,
				channel_id,
				query,
			},
			cx,
		);
		cx.notify();
	}

	fn jump_to(&mut self, channel: Snowflake, message: Snowflake, cx: &mut Context<Self>) {
		if Some(channel) == self.channel {
			self.timeline
				.update(cx, |timeline, cx| timeline.jump_to(message, cx));
			return;
		}
		self.pending_jump = Some(message);
		let guild = self.data.read(cx).state().channel_guild(channel);
		self.select(guild, channel, cx);
	}

	fn on_timeline(&mut self, event: &TimelineEvent, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			TimelineEvent::OpenLink(url) => links::open_link(url, &self.backend, window, cx),
			TimelineEvent::OpenImage(target) => {
				self.overlay = Some(Overlay::Lightbox(target.clone()));
				cx.notify();
			}
			TimelineEvent::Reply(message) => self.start_reply(*message, window, cx),
			TimelineEvent::OpenEmoji { message_id, at } => {
				if let Some(channel) = self.channel {
					let target = PickerTarget::Reaction {
						channel,
						message: *message_id,
					};
					self.community.update(cx, |community, cx| {
						community.open_picker(PickerTab::Emoji, target, *at, window, cx)
					});
				}
			}
			TimelineEvent::OpenProfile { user, at } => self.open_profile(*user, *at, Anchor::TopLeft, cx),
		}
	}

	fn start_reply(&mut self, message: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let name = {
			let data = self.data.read(cx);
			let state = data.state();
			state
				.messages(channel)
				.and_then(|cache| cache.by_id.get(&message))
				.map(|message| state.display_name(self.guild, message.author.id))
		};
		if let Some(name) = name {
			self.composer.update(cx, |composer, cx| {
				composer.set_reply(message, name.into(), window, cx)
			});
		}
	}

	fn open_profile(&mut self, user: Snowflake, at: Point<Pixels>, anchor: Anchor, cx: &mut Context<Self>) {
		let data = self.data.clone();
		let guild = self.guild;
		let card = cx.new(|cx| ProfileCard::new(data, user, guild, cx));
		let subscription = cx.subscribe(&card, |this, _, event: &ProfileEvent, cx| {
			if let ProfileEvent::OpenFull { user, guild } = event {
				this.account
					.update(cx, |account, cx| account.open_full_profile(*user, *guild, cx));
			}
			this.overlay = None;
			cx.notify();
		});
		self._subscriptions.push(subscription);
		self.overlay = Some(Overlay::Profile { card, at, anchor });
		cx.notify();
	}

	fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.account.update(cx, |account, cx| account.dismiss(cx)) {
			return;
		}
		if self.plugin_ui.update(cx, |ui, cx| ui.dismiss_top(cx)) {
			return;
		}
		if self
			.community
			.update(cx, |community, cx| community.dismiss_top(cx))
		{
			return;
		}
		if self.overlay.take().is_some() {
			window.focus(&self.focus, cx);
			cx.notify();
		} else if self.settings.is_some() {
			self.close_settings(window, cx);
		} else {
			self.community.update(cx, |community, cx| community.escape(cx));
		}
	}

	fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.settings.is_some() {
			return;
		}
		let backend = self.backend.clone();
		let snapshot = (backend.read)();
		let data = self.data.clone();
		let view = cx.new(|cx| SettingsWindow::new(snapshot, backend, data, window, cx));
		cx.subscribe_in(
			&view,
			window,
			|this, _, event: &SettingsEvent, window, cx| match event {
				SettingsEvent::Close => this.close_settings(window, cx),
				SettingsEvent::LogOut => {
					this.send(Command::Logout, cx);
					this.close_settings(window, cx);
					cx.emit(ShellEvent::LoggedOut);
				}
				SettingsEvent::CompactMessages(compact) => {
					let compact = *compact;
					this.timeline
						.update(cx, |timeline, cx| timeline.set_compact(compact, cx));
				}
			},
		)
		.detach();
		self.settings = Some(view);
		cx.notify();
	}

	fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.settings.take().is_some() {
			window.focus(&self.focus, cx);
			cx.notify();
		}
	}

	fn navigate(&mut self, direction: NavigationDirection, cx: &mut Context<Self>) {
		let place = match direction {
			NavigationDirection::Back => self.history.back(),
			NavigationDirection::Forward => self.history.forward(),
		};
		if let Some(place) = place {
			self.select(place.guild, place.channel, cx);
			cx.notify();
		}
	}

	fn history_button(&self, direction: NavigationDirection, cx: &mut Context<Self>) -> AnyElement {
		let (id, icon, label, enabled) = match direction {
			NavigationDirection::Back => (
				"title-back",
				IconKind::Back,
				"title-back",
				self.history.can_go_back(),
			),
			NavigationDirection::Forward => (
				"title-forward",
				IconKind::Ahead,
				"title-forward",
				self.history.can_go_forward(),
			),
		};
		let muted = cx.global::<Theme>().interactive_muted;
		IconButton::new(id, icon)
			.icon_size(rem(18.))
			.hit_size(rem(24.))
			.tooltip(cx.t(label))
			.when(!enabled, |button| button.color(muted))
			.on_click(cx.listener(move |this, _, _, cx| this.navigate(direction, cx)))
			.into_any_element()
	}

	fn title_bar(&self, cx: &mut Context<Self>) -> TitleBar {
		let context = self.guild.and_then(|guild| {
			let data = self.data.read(cx);
			let state = data.state();
			let guild = state.guild(guild)?;
			Some(TitleContext {
				name: guild.name.clone().into(),
				icon: guild
					.icon
					.as_deref()
					.map(|hash| media::cdn::guild_icon_url(guild.id.get(), hash, 64)),
			})
		});
		let inbox = IconButton::new("title-inbox", IconKind::Inbox)
			.icon_size(rem(18.))
			.hit_size(rem(24.))
			.tooltip(cx.t("top-bar-inbox"))
			.on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
				let at = event.position();
				this.community
					.update(cx, |community, cx| community.open_inbox(at, window, cx));
			}));
		let help = IconButton::new("title-help", IconKind::Help)
			.icon_size(rem(18.))
			.hit_size(rem(24.))
			.tooltip(cx.t("top-bar-help"))
			.on_click(cx.listener(|this, _, window, cx| {
				this.community
					.update(cx, |community, cx| community.open_help(window, cx));
			}));
		let bar = TitleBar::new()
			.leading([
				self.history_button(NavigationDirection::Back, cx),
				self.history_button(NavigationDirection::Forward, cx),
			])
			.trailing([inbox.into_any_element(), help.into_any_element()]);
		match context {
			Some(context) => bar.context(context),
			None => bar,
		}
	}

	fn room_visible(&self, cx: &gpui::App) -> bool {
		let Some(channel) = self.channel else {
			return false;
		};
		let data = self.data.read(cx);
		let state = data.state();
		state.voice().is_some_and(|voice| voice.channel_id == channel)
			|| state.channel(channel).is_some_and(|found| {
				matches!(found.kind, ChannelType::GuildVoice | ChannelType::GuildStageVoice)
			})
	}

	fn start_call(&mut self, video: bool, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let in_call = self
			.data
			.read(cx)
			.state()
			.voice()
			.is_some_and(|voice| voice.channel_id == channel);
		if in_call {
			return;
		}
		if video {
			self.voice_room.update(cx, |room, _| room.camera_when_connected());
		}
		self.send(Command::StartCall { channel_id: channel }, cx);
	}

	fn open_voice_menu(&mut self, user: Snowflake, at: Point<Pixels>, cx: &mut Context<Self>) {
		let data = self.data.clone();
		let name: SharedString = self.data.read(cx).state().display_name(self.guild, user).into();
		let menu = cx.new(|cx| UserMenu::new(data, user, name, cx));
		let subscription = cx.subscribe(&menu, move |this, _, event: &UserMenuEvent, cx| {
			let UserMenuEvent::Profile(user) = event;
			this.open_profile(*user, at, Anchor::TopLeft, cx);
		});
		self._subscriptions.push(subscription);
		self.overlay = Some(Overlay::VoiceUser { menu, at });
		cx.notify();
	}

	fn switcher_entries(&self, cx: &gpui::App) -> Vec<SwitcherEntry> {
		let data = self.data.read(cx);
		let state = data.state();
		let mut entries = Vec::new();
		for summary in state.guild_summaries() {
			for node in state.guild_channel_tree(summary.id) {
				for channel in node.children.iter().filter(|channel| {
					!channel.locked
						&& !matches!(channel.kind, ChannelType::GuildForum | ChannelType::GuildMedia)
				}) {
					let voice = matches!(
						channel.kind,
						ChannelType::GuildVoice | ChannelType::GuildStageVoice
					);
					entries.push(SwitcherEntry {
						target: SwitcherTarget::Channel {
							guild: Some(summary.id),
							channel: channel.id,
						},
						name: channel.name.clone().into(),
						context: Some(summary.name.clone().into()),
						leading: SwitcherLeading::Icon(channel_icon(channel.kind)),
						category: if voice {
							SwitcherCategory::Voice
						} else {
							SwitcherCategory::Channels
						},
					});
				}
			}
		}
		let rows = state.dm_rows();
		let dm_users: Vec<Snowflake> = rows
			.iter()
			.filter(|row| row.recipients.len() == 1)
			.map(|row| row.recipients[0])
			.collect();
		entries.extend(rows.into_iter().map(|row| {
			let leading = match row.recipients.as_slice() {
				[user] => SwitcherLeading::Avatar(crate::lookup::avatar_for(&state, *user, 64)),
				_ => SwitcherLeading::Icon(IconKind::Group),
			};
			SwitcherEntry {
				target: SwitcherTarget::Channel {
					guild: None,
					channel: row.channel_id,
				},
				name: row.name.into(),
				context: None,
				leading,
				category: SwitcherCategory::DirectMessages,
			}
		}));
		entries.extend(
			state
				.friends()
				.into_iter()
				.filter(|friend| !dm_users.contains(&friend.user_id))
				.map(|friend| SwitcherEntry {
					target: SwitcherTarget::Friend(friend.user_id),
					leading: SwitcherLeading::Avatar(media::cdn::avatar_url(
						friend.user_id.get(),
						friend.avatar.as_deref(),
						None,
						64,
					)),
					name: friend.display.into(),
					context: Some(friend.username.into()),
					category: SwitcherCategory::Friends,
				}),
		);
		entries.extend(state.guild_summaries().into_iter().map(|summary| {
			SwitcherEntry {
				target: SwitcherTarget::Guild(summary.id),
				leading: summary
					.icon
					.as_deref()
					.map_or(SwitcherLeading::Icon(IconKind::Home), |hash| {
						SwitcherLeading::Avatar(media::cdn::guild_icon_url(summary.id.get(), hash, 64))
					}),
				name: summary.name.into(),
				context: None,
				category: SwitcherCategory::Servers,
			}
		}));
		entries
	}

	fn open_quick_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.settings.is_some() || window.has_active_dialog(cx) {
			return;
		}
		let entries = self.switcher_entries(cx);
		let recent = self.recent_targets.clone();
		let switcher = cx.new(|cx| QuickSwitcher::new(entries, recent, window, cx));
		cx.subscribe_in(
			&switcher,
			window,
			|this, _, event: &QuickSwitcherEvent, window, cx| {
				let QuickSwitcherEvent::Pick(target) = event;
				window.close_dialog(cx);
				match target {
					SwitcherTarget::Channel { guild, channel } => this.select(*guild, *channel, cx),
					SwitcherTarget::Guild(guild) => this.show_guild(*guild, window, cx),
					SwitcherTarget::Friend(user) => {
						this.home_pages.close(cx);
						this.send(Command::OpenDm { user_id: *user }, cx);
					}
				}
			},
		)
		.detach();
		let initial_focus = std::cell::Cell::new(true);
		window.open_dialog(cx, move |dialog, window, cx| {
			if initial_focus.replace(false) {
				switcher.update(cx, |switcher, cx| switcher.focus(window, cx));
			}
			dialog
				.w(rem(560.).to_pixels(window.rem_size()))
				.margin_top(rem(96.).to_pixels(window.rem_size()))
				.close_button(false)
				.on_ok(|_, _, _| false)
				.child(switcher.clone())
		});
	}

	fn overlay_element(
		&self,
		window: &Window,
		theme: &Theme,
		cx: &mut Context<Self>,
	) -> Option<gpui::AnyElement> {
		let overlay = self.overlay.as_ref()?;
		Some(match overlay {
			Overlay::Lightbox(target) => {
				let weak = cx.weak_entity();
				let close = Rc::new(move |window: &mut Window, cx: &mut gpui::App| {
					weak.update(cx, |this, cx| this.close_overlay(window, cx)).ok();
				});
				lightbox::lightbox(target, &self.backend, window, theme, cx, close)
			}
			Overlay::Profile { card, at, anchor } => {
				floating(card.clone().into_any_element(), *at, *anchor, cx)
			}
			Overlay::VoiceUser { menu, at } => {
				floating(menu.clone().into_any_element(), *at, Anchor::TopLeft, cx)
			}
		})
	}
}

fn floating(
	content: gpui::AnyElement,
	at: Point<Pixels>,
	anchor: Anchor,
	cx: &mut Context<Shell>,
) -> gpui::AnyElement {
	deferred(
		anchored()
			.position(at)
			.anchor(anchor)
			.snap_to_window_with_margin(cx.global::<Theme>().pixels(rem(8.)))
			.child(
				div()
					.occlude()
					.on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_overlay(window, cx)))
					.child(content),
			),
	)
	.with_priority(1)
	.into_any_element()
}

trait PresenceChanged {
	fn presence_any(&self) -> bool;
}

impl PresenceChanged for Changes {
	fn presence_any(&self) -> bool {
		!self.presence.is_empty()
	}
}

impl Render for Shell {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = Theme::current(cx);
		let width = window.viewport_size().width;
		let rem_size = window.rem_size();
		let in_guild = self.guild.is_some();
		let show_sidebar = width >= rem(SIDEBAR_MIN_WIDTH).to_pixels(rem_size);
		let room = self.room_visible(cx);
		let show_plugin_panel =
			plugins::right_panel_visible(cx) && width >= rem(SIDEBAR_MIN_WIDTH).to_pixels(rem_size);
		let status_bar = plugins::status_bar(cx);
		let forum = self.community.read(cx).forum_view(cx);
		let thread_panel = self.community.read(cx).thread_panel(cx);
		let thread_full = thread_panel
			.as_ref()
			.is_some_and(|panel| panel.read(cx).is_full());
		let show_members = in_guild
			&& !room && thread_panel.is_none()
			&& !show_plugin_panel
			&& self.panel == RightPanel::Members
			&& self.members_visible
			&& width >= rem(MEMBER_LIST_MIN_WIDTH).to_pixels(rem_size);
		let in_group = self.groups.active(cx);
		let show_group_members = in_group
			&& self.panel == RightPanel::Members
			&& self.members_visible
			&& width >= rem(MEMBER_LIST_MIN_WIDTH).to_pixels(rem_size);
		let show_search =
			self.panel == RightPanel::Search && width >= rem(SIDEBAR_MIN_WIDTH).to_pixels(rem_size);
		let sidebar = if in_guild {
			self.channel_sidebar.clone().into_any_element()
		} else {
			self.dm_sidebar.clone().into_any_element()
		};
		let overlay = self.overlay_element(window, &theme, cx);
		let home = self.home_pages.element(&theme);
		let plugin_page = plugins::open_page(cx).and_then(|page| plugins::main_view(&page, cx));
		cx.global::<crate::playback::Playback>().set_shell_hidden(
			room || plugin_page.is_some()
				|| forum.is_some()
				|| overlay.is_some()
				|| self.settings.is_some()
				|| thread_panel.is_some(),
		);
		div()
			.key_context(SHELL_CONTEXT)
			.track_focus(&self.focus)
			.on_action(
				cx.listener(|this, _: &OpenQuickSwitcher, window, cx| this.open_quick_switcher(window, cx)),
			)
			.on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)))
			.on_action(cx.listener(|this, _: &CloseOverlay, window, cx| this.close_overlay(window, cx)))
			.on_action(cx.listener(|this, _: &PreviousChannel, _, cx| this.step_channel(false, cx)))
			.on_action(cx.listener(|this, _: &NextChannel, _, cx| this.step_channel(true, cx)))
			.on_mouse_down(
				MouseButton::Navigate(NavigationDirection::Back),
				cx.listener(|this, _, _, cx| this.navigate(NavigationDirection::Back, cx)),
			)
			.on_mouse_down(
				MouseButton::Navigate(NavigationDirection::Forward),
				cx.listener(|this, _, _, cx| this.navigate(NavigationDirection::Forward, cx)),
			)
			.relative()
			.flex()
			.flex_col()
			.size_full()
			.bg(theme.background_tertiary)
			.text_color(theme.text_normal)
			.text_size(theme.font_md)
			.font_family(theme.ui_font.clone())
			.child(self.title_bar(cx))
			.child(
				div()
					.flex()
					.flex_1()
					.min_h_0()
					.map(|body| {
						if show_sidebar {
							body.child(
								div()
									.flex()
									.flex_col()
									.flex_none()
									.h_full()
									.child(
										div().flex().flex_1().min_h_0().child(self.rail.clone()).child(
											div()
												.flex()
												.flex_col()
												.flex_none()
												.w(theme.channel_sidebar_width)
												.h_full()
												.rounded_tl(theme.radius_md)
												.border_t_1()
												.border_l_1()
												.border_color(theme.divider)
												.bg(theme.background_tertiary)
												.overflow_hidden()
												.child(
													div()
														.flex()
														.flex_col()
														.flex_1()
														.min_h_0()
														.overflow_hidden()
														.child(sidebar),
												)
												.child(SidebarPanel),
										),
									)
									.child(
										div()
											.flex()
											.flex_col()
											.flex_none()
											.m(theme.space_8)
											.rounded(theme.radius_md)
											.border_1()
											.border_color(theme.divider)
											.bg(theme.user_panel_background)
											.overflow_hidden()
											.child(self.voice_panel.clone())
											.child(self.user_panel.clone()),
									),
							)
						} else {
							body.child(self.rail.clone())
						}
					})
					.child(
						div()
							.flex()
							.flex_col()
							.relative()
							.flex_1()
							.min_w_0()
							.h_full()
							.bg(theme.background_primary)
							.border_t_1()
							.border_l_1()
							.border_color(theme.divider)
							.when(!show_sidebar, |column| column.rounded_tl(theme.radius_md))
							.child(self.top_bar.clone())
							.child(
								div()
									.flex()
									.flex_1()
									.min_h_0()
									.when(!thread_full, |main| {
										main.child(
											div()
												.flex()
												.flex_col()
												.flex_1()
												.min_w_0()
												.min_h_0()
												.when(room, |column| column.child(self.voice_room.clone()))
												.when(!room, |column| match (plugin_page, forum) {
													(Some(page), _) => {
														column.child(page).children(status_bar)
													}
													(None, Some(forum)) => column.child(forum),
													(None, None) => column
														.child(self.timeline.clone())
														.child(self.composer.clone())
														.children(status_bar),
												}),
										)
									})
									.children(thread_panel)
									.when(show_members, |main| main.child(self.member_list.clone()))
									.when(show_group_members, |main| main.child(self.groups.members.clone()))
									.when(show_plugin_panel, |main| main.child(PluginRightPanel))
									.when(show_search, |main| main.child(self.search_panel.clone())),
							)
							.children(home),
					),
			)
			.child(DebugPanel)
			.child(self.call_toast.clone())
			.children(self.settings.clone())
			.child(self.account.clone())
			.children(overlay)
			.child(self.community.clone())
			.child(self.plugin_ui.clone())
	}
}
