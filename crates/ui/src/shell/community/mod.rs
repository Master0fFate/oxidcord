pub mod access;
pub mod add_server;
pub mod art;
pub mod files;
pub mod forward;
pub mod invites;
pub mod menus;
pub mod modal;
pub mod notify;
pub mod raw;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui::Focusable as _;
use gpui::{
	Anchor, AnyElement, AppContext as _, Context, Entity, EventEmitter, Global, InteractiveElement as _,
	IntoElement, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _, Subscription, Window,
	anchored, deferred, div, point, px,
};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use protocol::{ChannelType, Snowflake};
use session::Command;
use state::{Changes, Selection};
use store::Settings;

use super::composer::{Composer, ComposerEvent};
use super::links;
use super::message::{ImageTarget, ReactionRequests};
use super::pickers::{Picker, PickerEvent, PickerTab, SkinTone};
use super::server_settings::{Page, ServerSettings, ServerSettingsEvent};
use super::settings::SettingsBackend;
use super::threads::{
	ForumEvent, ForumView, InboxEvent, InboxPanel, PinsEvent, PinsPanel, ThreadList, ThreadListEvent,
	ThreadPanel, ThreadPanelEvent, create, pins, post,
};
use super::timeline::TimelineEvent;
use crate::data::Data;
use crate::theme::rem;

const POPOVER_GAP: f32 = 12.;
const HELP_URL: &str = "https://support.discord.com";

#[derive(Clone)]
pub struct CommunityHandle {
	pub hub: Entity<Community>,
	pub data: Entity<Data>,
	pub backend: Rc<SettingsBackend>,
}

impl Global for CommunityHandle {}

pub fn act(
	window: &mut Window,
	cx: &mut gpui::App,
	run: impl FnOnce(&mut Community, &mut Window, &mut Context<Community>),
) {
	let hub = cx.global::<CommunityHandle>().hub.clone();
	hub.update(cx, |community, cx| run(community, window, cx));
}

pub fn data_handle(cx: &gpui::App) -> Entity<Data> {
	cx.global::<CommunityHandle>().data.clone()
}

pub fn settings(cx: &gpui::App) -> Settings {
	(cx.global::<CommunityHandle>().backend.read)()
}

pub fn developer_mode(cx: &gpui::App) -> bool {
	settings(cx).developer.developer_mode
}

pub enum CommunityEvent {
	OpenImage(ImageTarget),
	OpenProfile { user: Snowflake, at: Point<Pixels> },
	Jump { channel: Snowflake, message: Snowflake },
	SearchIn(Snowflake),
	Layout,
}

#[derive(Clone, Copy)]
pub enum ComposerSlot {
	Main,
	Thread,
}

#[derive(Clone, Copy)]
pub enum PickerTarget {
	Composer(ComposerSlot),
	Reaction { channel: Snowflake, message: Snowflake },
}

enum Content {
	Threads(Entity<ThreadList>),
	Pins(Entity<PinsPanel>),
	Inbox(Entity<InboxPanel>),
	Picker(Entity<Picker>),
}

struct Floating {
	content: Content,
	at: Point<Pixels>,
	anchor: Anchor,
}

pub struct Community {
	data: Entity<Data>,
	backend: Rc<SettingsBackend>,
	main_composer: Entity<Composer>,
	guild: Option<Snowflake>,
	channel: Option<Snowflake>,
	panel: Entity<ThreadPanel>,
	forum: Entity<ForumView>,
	floating: Option<Floating>,
	settings: Option<Entity<ServerSettings>>,
	seen_rejection: u64,
	plugin_slot: ComposerSlot,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<CommunityEvent> for Community {}

fn thread_parent(state: &state::State, channel: Snowflake) -> Option<Snowflake> {
	let found = state.channel(channel)?;
	matches!(
		found.kind,
		ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::AnnouncementThread
	)
	.then_some(found.parent_id)
	.flatten()
}

fn is_forum(kind: ChannelType) -> bool {
	matches!(kind, ChannelType::GuildForum | ChannelType::GuildMedia)
}

impl Community {
	pub fn new(
		data: Entity<Data>,
		backend: Rc<SettingsBackend>,
		main_composer: Entity<Composer>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let compact = (backend.read)().appearance.compact_messages;
		let panel = cx.new(|cx| ThreadPanel::new(data.clone(), compact, window, cx));
		let forum = cx.new(|_| ForumView::new(data.clone()));
		let mut subscriptions = Vec::new();
		subscriptions.push(
			cx.subscribe_in(
				&panel,
				window,
				|this, _, event: &ThreadPanelEvent, _, cx| match event {
					ThreadPanelEvent::Close => this.close_thread(cx),
					ThreadPanelEvent::Layout => cx.emit(CommunityEvent::Layout),
				},
			),
		);
		let thread_composer = panel.read(cx).composer().clone();
		subscriptions.push(cx.subscribe_in(
			&thread_composer,
			window,
			|this, _, event: &ComposerEvent, window, cx| match event {
				ComposerEvent::OpenPicker { tab, at } => this.open_picker(
					*tab,
					PickerTarget::Composer(ComposerSlot::Thread),
					*at,
					window,
					cx,
				),
				ComposerEvent::Sent => {
					let timeline = this.panel.read(cx).timeline().clone();
					timeline.update(cx, |timeline, cx| timeline.scroll_to_end(cx));
				}
			},
		));
		let timeline = panel.read(cx).timeline().clone();
		subscriptions.push(cx.subscribe_in(
			&timeline,
			window,
			|this, _, event: &TimelineEvent, window, cx| this.on_thread_timeline(event, window, cx),
		));
		subscriptions.push(
			cx.subscribe_in(
				&forum,
				window,
				|this, _, event: &ForumEvent, window, cx| match event {
					ForumEvent::OpenPost(id) => this.open_thread(*id, cx),
					ForumEvent::NewPost(channel) => post::open(window, cx, this.data.clone(), *channel),
				},
			),
		);
		Self {
			data,
			backend,
			main_composer,
			guild: None,
			channel: None,
			panel,
			forum,
			floating: None,
			settings: None,
			seen_rejection: 0,
			plugin_slot: ComposerSlot::Main,
			_subscriptions: subscriptions,
		}
	}

	pub fn install(self_entity: &Entity<Self>, data: Entity<Data>, cx: &mut gpui::App) {
		cx.set_global(SkinTone(Rc::new(Cell::new(0))));
		cx.set_global(invites::InviteRequests::default());
		cx.set_global(ReactionRequests::default());
		cx.set_global(CommunityHandle {
			hub: self_entity.clone(),
			data,
			backend: self_entity.read(cx).backend.clone(),
		});
	}

	fn send(&self, command: Command, cx: &gpui::App) {
		self.data.read(cx).send(command);
	}

	pub fn show_main(
		&mut self,
		guild: Option<Snowflake>,
		channel: Option<Snowflake>,
		cx: &mut Context<Self>,
	) {
		self.guild = guild;
		self.channel = channel;
		let forum = channel.filter(|id| {
			self.data
				.read(cx)
				.state()
				.channel(*id)
				.is_some_and(|found| is_forum(found.kind))
		});
		self.forum.update(cx, |view, cx| view.show(guild, forum, cx));
		cx.notify();
	}

	pub fn forum_view(&self, cx: &gpui::App) -> Option<Entity<ForumView>> {
		self.forum.read(cx).channel().map(|_| self.forum.clone())
	}

	pub fn plugin_composer(&self, cx: &gpui::App) -> Entity<Composer> {
		match (self.plugin_slot, self.thread_panel(cx)) {
			(ComposerSlot::Thread, Some(panel)) => panel.read(cx).composer().clone(),
			_ => self.main_composer.clone(),
		}
	}

	pub fn thread_panel(&self, cx: &gpui::App) -> Option<Entity<ThreadPanel>> {
		self.panel.read(cx).thread().map(|_| self.panel.clone())
	}

	pub fn route(&mut self, selected: Selection, window: &mut Window, cx: &mut Context<Self>) -> Selection {
		let parent = thread_parent(&self.data.read(cx).state(), selected.channel_id);
		match parent {
			Some(parent) => {
				self.show_thread(selected.guild_id, Some(selected.channel_id), window, cx);
				Selection {
					guild_id: selected.guild_id,
					channel_id: parent,
				}
			}
			None => {
				self.show_thread(selected.guild_id, None, window, cx);
				selected
			}
		}
	}

	fn show_thread(
		&mut self,
		guild: Option<Snowflake>,
		thread: Option<Snowflake>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.panel.read(cx).thread() == thread {
			return;
		}
		let compact = (self.backend.read)().appearance.compact_messages;
		self.panel
			.update(cx, |panel, cx| panel.show(guild, thread, compact, window, cx));
		cx.notify();
	}

	pub fn open_thread(&mut self, thread: Snowflake, cx: &mut Context<Self>) {
		let (guild, parent) = {
			let data = self.data.read(cx);
			let state = data.state();
			(state.channel_guild(thread), thread_parent(&state, thread))
		};
		if let Some(parent) = parent
			&& self.channel != Some(parent)
		{
			self.send(
				Command::SelectChannel {
					guild_id: guild,
					channel_id: parent,
				},
				cx,
			);
		}
		self.send(
			Command::SelectChannel {
				guild_id: guild,
				channel_id: thread,
			},
			cx,
		);
		self.floating = None;
		cx.notify();
	}

	pub fn close_thread(&mut self, cx: &mut Context<Self>) {
		if let Some(channel) = self.channel {
			self.send(
				Command::SelectChannel {
					guild_id: self.guild,
					channel_id: channel,
				},
				cx,
			);
		}
	}

	pub fn open_threads(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
		let (Some(guild), Some(channel)) = (self.guild, self.channel) else {
			return;
		};
		let target = {
			let data = self.data.read(cx);
			let state = data.state();
			if is_forum_channel(&state, channel) {
				None
			} else {
				Some(channel)
			}
		};
		let Some(channel) = target else {
			return;
		};
		let list = cx.new(|cx| ThreadList::new(self.data.clone(), guild, channel, cx));
		self._subscriptions.push(cx.subscribe_in(
			&list,
			window,
			move |this, _, event: &ThreadListEvent, window, cx| match event {
				ThreadListEvent::Open(id) => this.open_thread(*id, cx),
				ThreadListEvent::Create => {
					this.floating = None;
					create::open(window, cx, this.data.clone(), channel, None);
					cx.notify();
				}
				ThreadListEvent::Close => {
					this.floating = None;
					cx.notify();
				}
			},
		));
		self.float(Content::Threads(list), at, window, cx);
	}

	pub fn search_in(&mut self, channel: Snowflake, cx: &mut Context<Self>) {
		cx.emit(CommunityEvent::SearchIn(channel));
	}

	pub fn open_pins(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.thread_or_channel(cx) else {
			return;
		};
		let panel = cx.new(|cx| PinsPanel::new(self.data.clone(), channel, cx));
		self._subscriptions.push(
			cx.subscribe_in(&panel, window, |this, _, event: &PinsEvent, _, cx| {
				match event {
					PinsEvent::Jump { channel, message } => cx.emit(CommunityEvent::Jump {
						channel: *channel,
						message: *message,
					}),
					PinsEvent::Close => {}
				}
				this.floating = None;
				cx.notify();
			}),
		);
		self.float(Content::Pins(panel), at, window, cx);
	}

	pub fn open_inbox(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
		let panel = cx.new(|cx| InboxPanel::new(self.data.clone(), cx));
		self._subscriptions.push(
			cx.subscribe_in(&panel, window, |this, _, event: &InboxEvent, _, cx| {
				match event {
					InboxEvent::Jump { channel, message } => cx.emit(CommunityEvent::Jump {
						channel: *channel,
						message: *message,
					}),
					InboxEvent::Close => {}
				}
				this.floating = None;
				cx.notify();
			}),
		);
		self.float(Content::Inbox(panel), at, window, cx);
	}

	pub fn open_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		links::open_link(HELP_URL, &self.backend, window, cx);
	}

	fn thread_or_channel(&self, cx: &gpui::App) -> Option<Snowflake> {
		self.panel.read(cx).thread().or(self.channel)
	}

	pub fn create_thread(
		&mut self,
		channel: Snowflake,
		message: Option<Snowflake>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		create::open(window, cx, self.data.clone(), channel, message);
	}

	pub fn open_forward(
		&mut self,
		channel: Snowflake,
		message: Snowflake,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		forward::open(window, cx, self.data.clone(), channel, message);
	}

	pub fn confirm_pin(
		&mut self,
		channel: Snowflake,
		message: Snowflake,
		pin: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		pins::confirm(window, cx, self.data.clone(), channel, message, pin);
	}

	pub fn open_picker(
		&mut self,
		tab: PickerTab,
		target: PickerTarget,
		at: Point<Pixels>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let (channel, full) = match target {
			PickerTarget::Composer(ComposerSlot::Main) => (self.channel, true),
			PickerTarget::Composer(ComposerSlot::Thread) => (self.panel.read(cx).thread(), true),
			PickerTarget::Reaction { channel, .. } => (Some(channel), false),
		};
		let Some(channel) = channel else {
			return;
		};
		let guild = self.data.read(cx).state().channel_guild(channel);
		let share_usage = (self.backend.read)().advanced.share_gif_usage;
		let data = self.data.clone();
		let picker = cx.new(|cx| {
			Picker::new(
				data,
				(guild, channel),
				if full { tab } else { PickerTab::Emoji },
				full,
				share_usage,
				window,
				cx,
			)
		});
		self._subscriptions.push(cx.subscribe_in(
			&picker,
			window,
			move |this, _, event: &PickerEvent, window, cx| match event {
				PickerEvent::Emoji(choice) => {
					this.floating = None;
					match target {
						PickerTarget::Composer(slot) => {
							let composer = match slot {
								ComposerSlot::Main => this.main_composer.clone(),
								ComposerSlot::Thread => this.panel.read(cx).composer().clone(),
							};
							let markup = choice.markup();
							composer.update(cx, |composer, cx| composer.insert(&markup, window, cx));
						}
						PickerTarget::Reaction { channel, message } => this.send(
							Command::AddReaction {
								channel_id: channel,
								message_id: message,
								emoji: choice.reaction_key(),
								burst: false,
							},
							cx,
						),
					}
					cx.notify();
				}
				PickerEvent::PluginPick { emoji, channel } => {
					this.floating = None;
					if let PickerTarget::Composer(slot) = target {
						this.plugin_slot = slot;
						this.send(
							Command::PluginEmojiPick {
								emoji: emoji.clone(),
								channel_id: *channel,
							},
							cx,
						);
					}
					cx.notify();
				}
				PickerEvent::Sent => {
					this.floating = None;
					cx.notify();
				}
			},
		));
		picker.update(cx, |picker, cx| picker.focus(window, cx));
		self.float(Content::Picker(picker), at, window, cx);
	}

	pub fn open_server_settings(
		&mut self,
		guild: Snowflake,
		page: Option<Page>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let data = self.data.clone();
		let view = cx.new(|cx| ServerSettings::new(data, guild, page, window, cx));
		self._subscriptions.push(cx.subscribe_in(
			&view,
			window,
			|this, _, event: &ServerSettingsEvent, _, cx| {
				let ServerSettingsEvent::Close = event;
				this.settings = None;
				cx.notify();
			},
		));
		let focus = view.focus_handle(cx);
		window.focus(&focus, cx);
		self.floating = None;
		self.settings = Some(view);
		cx.notify();
	}

	pub fn escape(&mut self, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let unread = {
			let data = self.data.read(cx);
			let state = data.state();
			state.read_state(channel).is_some_and(|read| {
				read.mention_count.unwrap_or(0) > 0
					|| state
						.channel(channel)
						.and_then(|found| found.last_message_id)
						.zip(read.last_message_id)
						.is_some_and(|(last, seen)| last > seen)
			})
		};
		if unread {
			self.send(Command::MarkChannelRead { channel_id: channel }, cx);
		}
	}

	fn float(&mut self, content: Content, at: Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
		let anchor = if at.y > window.viewport_size().height / 2. {
			Anchor::BottomRight
		} else {
			Anchor::TopRight
		};
		self.floating = Some(Floating { content, at, anchor });
		cx.notify();
	}

	pub fn dismiss_top(&mut self, cx: &mut Context<Self>) -> bool {
		if self.floating.take().is_some() || self.settings.take().is_some() {
			cx.notify();
			return true;
		}
		false
	}

	pub fn on_changes(&mut self, changes: &Changes, window: &mut Window, cx: &mut Context<Self>) {
		self.panel.update(cx, |panel, cx| {
			panel
				.timeline()
				.update(cx, |timeline, cx| timeline.on_changes(changes, window, cx));
			if changes.session {
				let connected = self.data.read(cx).state().session().connected;
				panel
					.timeline()
					.update(cx, |timeline, cx| timeline.set_connected(connected, cx));
			}
		});
		let thread = self.panel.read(cx).thread();
		if let Some(thread) = thread {
			if changes.typing.contains(thread) {
				self.sync_thread_typing(thread, cx);
			}
			if changes.drafts.contains(thread) {
				let composer = self.panel.read(cx).composer().clone();
				composer.update(cx, |composer, cx| composer.on_draft(window, cx));
			}
		}
		if changes.rejected {
			self.show_rejection(window, cx);
		}
		if changes.channel_lists.is_empty()
			&& changes.loaded.is_empty()
			&& !changes.guild_list
			&& !changes.read_state
		{
			return;
		}
		self.forum.update(cx, |forum, cx| forum.refresh(cx));
		if let Some(floating) = &self.floating {
			match &floating.content {
				Content::Threads(list) => list.update(cx, |list, cx| list.refresh(cx)),
				Content::Pins(pins) => pins.update(cx, |pins, cx| pins.refresh(cx)),
				Content::Inbox(inbox) => inbox.update(cx, |inbox, cx| inbox.refresh(cx)),
				Content::Picker(picker) => picker.update(cx, |picker, cx| picker.refresh(cx)),
			}
		}
		if thread.is_some() {
			self.panel.update(cx, |_, cx| cx.notify());
		}
	}

	pub fn on_media_ready(&mut self, cx: &mut Context<Self>) {
		self.forum.update(cx, |_, cx| cx.notify());
		self.panel.update(cx, |panel, cx| {
			panel.timeline().update(cx, |_, cx| cx.notify());
			panel.composer().update(cx, |_, cx| cx.notify());
		});
		if let Some(floating) = &self.floating
			&& let Content::Picker(picker) = &floating.content
		{
			picker.update(cx, |picker, cx| picker.refresh(cx));
		}
		cx.notify();
	}

	fn sync_thread_typing(&mut self, thread: Snowflake, cx: &mut Context<Self>) {
		let names: Vec<SharedString> = {
			let data = self.data.read(cx);
			let state = data.state();
			state
				.typing_users(thread, Instant::now())
				.into_iter()
				.map(|user| state.display_name(self.guild, user).into())
				.collect()
		};
		let composer = self.panel.read(cx).composer().clone();
		composer.update(cx, |composer, cx| composer.set_typing(names, cx));
	}

	fn show_rejection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let rejection = self.data.read(cx).state().last_rejection().cloned();
		let Some(rejection) = rejection.filter(|rejection| rejection.id > self.seen_rejection) else {
			return;
		};
		self.seen_rejection = rejection.id;
		window.push_notification(Notification::error(rejection.reason), cx);
	}

	fn on_thread_timeline(&mut self, event: &TimelineEvent, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			TimelineEvent::OpenLink(url) => links::open_link(url, &self.backend, window, cx),
			TimelineEvent::OpenImage(target) => cx.emit(CommunityEvent::OpenImage(target.clone())),
			TimelineEvent::OpenProfile { user, at } => {
				cx.emit(CommunityEvent::OpenProfile { user: *user, at: *at })
			}
			TimelineEvent::Reply(message) => self.reply_in_thread(*message, window, cx),
			TimelineEvent::OpenEmoji { message_id, at } => {
				if let Some(channel) = self.panel.read(cx).thread() {
					let target = PickerTarget::Reaction {
						channel,
						message: *message_id,
					};
					self.open_picker(PickerTab::Emoji, target, *at, window, cx);
				}
			}
		}
	}

	fn reply_in_thread(&mut self, message: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let Some(thread) = self.panel.read(cx).thread() else {
			return;
		};
		let name = {
			let data = self.data.read(cx);
			let state = data.state();
			state
				.messages(thread)
				.and_then(|cache| cache.by_id.get(&message))
				.map(|found| state.display_name(self.guild, found.author.id))
		};
		if let Some(name) = name {
			let composer = self.panel.read(cx).composer().clone();
			composer.update(cx, |composer, cx| {
				composer.set_reply(message, name.into(), true, window, cx)
			});
		}
	}

	fn floating_element(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
		let floating = self.floating.as_ref()?;
		let lift = if floating.anchor == Anchor::BottomRight {
			-POPOVER_GAP
		} else {
			POPOVER_GAP
		};
		let content: AnyElement = match &floating.content {
			Content::Threads(list) => list.clone().into_any_element(),
			Content::Pins(panel) => panel.clone().into_any_element(),
			Content::Inbox(panel) => panel.clone().into_any_element(),
			Content::Picker(picker) => picker.clone().into_any_element(),
		};
		let rem_size = window.rem_size();
		Some(
			deferred(
				anchored()
					.position(floating.at + point(px(0.), rem(lift).to_pixels(rem_size)))
					.anchor(floating.anchor)
					.snap_to_window_with_margin(rem(8.).to_pixels(rem_size))
					.child(
						div()
							.occlude()
							.on_mouse_down_out(cx.listener(|this, _, _, cx| {
								this.floating = None;
								cx.notify();
							}))
							.child(content),
					),
			)
			.with_priority(1)
			.into_any_element(),
		)
	}
}

fn is_forum_channel(state: &state::State, channel: Snowflake) -> bool {
	state.channel(channel).is_some_and(|found| is_forum(found.kind))
}

impl Render for Community {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.absolute()
			.top_0()
			.left_0()
			.size_full()
			.children(self.settings.clone())
			.children(self.floating_element(window, cx))
	}
}
