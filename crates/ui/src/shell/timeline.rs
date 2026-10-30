use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
	AppContext as _, ClickEvent, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, ListOffset, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _,
	Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::Sizable as _;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Escape, InputEvent, TextareaState};
use lru::LruCache;
use plugin_host::api::model::ClickTarget;
use protocol::{ChannelType, Snowflake};
use session::{Command, ReplyTarget};
use state::{Changes, MessageRow};

use super::community::act;
use super::inline_emoji::{self, EmojiTokens};
use super::message::{
	Dispatch, DocumentCache, Editing, FLASH_DURATION, ImageTarget, MessageAccess, Prefs, RowAction, RowEnv,
	Welcome, render_row,
};
use super::moderation;
use super::plugins::{self, LinkOrigin, PluginBridge};
use crate::components::{ChatList, ChatListState};
use crate::data::Data;
use crate::i18n::Translate as _;
use crate::icons::IconKind;
use crate::lookup::Lookup;
use crate::playback::Playback;
use crate::theme::{Theme, rem};

const NEAR_TOP_ROWS: usize = 14;
const SHORT_LIST_ROWS: usize = 14;
const OLDER_RETRY: Duration = Duration::from_secs(3);
const DOCUMENT_CACHE: usize = 256;
const GROUP_LOOKBACK: usize = 6;

pub enum TimelineEvent {
	OpenLink(String, LinkOrigin),
	OpenImage(ImageTarget),
	Reply(Snowflake),
	OpenEmoji {
		message_id: Snowflake,
		at: Point<Pixels>,
	},
	OpenProfile {
		user: Snowflake,
		at: Point<Pixels>,
	},
}

pub struct Timeline {
	data: Entity<Data>,
	scroll: Entity<ChatListState>,
	channel: Option<Snowflake>,
	guild: Option<Snowflake>,
	rows: Rc<Vec<MessageRow>>,
	welcome: Rc<Welcome>,
	revealed: Rc<RefCell<HashSet<(u64, usize)>>>,
	poll_picks: Rc<RefCell<HashMap<Snowflake, Vec<i64>>>>,
	documents: Rc<DocumentCache>,
	editing: Option<Editing>,
	selection: Rc<HashSet<Snowflake>>,
	compact: bool,
	connected: bool,
	requested_older: Option<(Snowflake, Instant)>,
	requested_newer: Option<(Snowflake, Instant)>,
	detached: bool,
	pending_jump: Option<Snowflake>,
	flash: Option<Snowflake>,
	flash_reset: Option<Task<()>>,
	scroll_anchor: Option<ListOffset>,
	own_update: bool,
	_subscriptions: Vec<Subscription>,
}

impl EventEmitter<TimelineEvent> for Timeline {}

fn same_row(a: &MessageRow, b: &MessageRow) -> bool {
	match (a, b) {
		(MessageRow::Message(x), MessageRow::Message(y)) => {
			(Arc::ptr_eq(&x.message, &y.message) || x.message == y.message)
				&& x.grouped_with_previous == y.grouped_with_previous
				&& x.pending == y.pending
				&& x.failed == y.failed
				&& x.author_color == y.author_color
				&& x.author_display == y.author_display
				&& x.author_badge == y.author_badge
				&& x.failure_reason == y.failure_reason
				&& x.plugin == y.plugin
				&& x.altered_by == y.altered_by
				&& x.deleted == y.deleted
				&& x.mentions_me == y.mentions_me
				&& x.member_roles == y.member_roles
				&& x.rendered.as_ref().map(Arc::as_ptr) == y.rendered.as_ref().map(Arc::as_ptr)
		}
		(MessageRow::DaySeparator(x), MessageRow::DaySeparator(y)) => x == y,
		(MessageRow::UnreadMarker, MessageRow::UnreadMarker)
		| (MessageRow::HistoryStart, MessageRow::HistoryStart)
		| (MessageRow::Loading, MessageRow::Loading) => true,
		_ => false,
	}
}

impl Timeline {
	pub fn new(data: Entity<Data>, compact: bool, cx: &mut Context<Self>) -> Self {
		let scroll = cx.new(|cx| ChatListState::new(0, cx));
		let observe = cx.observe(&scroll, |this, _, cx| this.on_scroll(cx));
		let rescale = cx.observe_global::<Theme>(|this, cx| {
			this.drive(cx, |scroll, cx| scroll.remeasure(cx));
			cx.notify();
		});
		let capacity = NonZeroUsize::new(DOCUMENT_CACHE).expect("capacity is not zero");
		Self {
			data,
			scroll,
			channel: None,
			guild: None,
			rows: Rc::new(Vec::new()),
			welcome: Rc::new(Welcome {
				name: SharedString::default(),
				direct: false,
			}),
			revealed: Rc::default(),
			poll_picks: Rc::default(),
			documents: Rc::new(RefCell::new(LruCache::new(capacity))),
			editing: None,
			selection: Rc::default(),
			compact,
			connected: false,
			requested_older: None,
			requested_newer: None,
			detached: false,
			pending_jump: None,
			flash: None,
			flash_reset: None,
			scroll_anchor: None,
			own_update: false,
			_subscriptions: vec![observe, rescale],
		}
	}

	pub fn set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
		if self.compact != compact {
			self.compact = compact;
			self.drive(cx, |scroll, cx| scroll.remeasure(cx));
			cx.notify();
		}
	}

	pub fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
		self.drive(cx, |scroll, cx| scroll.scroll_to_end(cx));
	}

	pub fn on_options(&mut self, cx: &mut Context<Self>) {
		cx.notify();
	}

	pub fn on_plugin_slots(&mut self, cx: &mut Context<Self>) {
		if cx.global::<PluginBridge>().accessory_changed(cx) {
			self.drive(cx, |scroll, cx| scroll.remeasure(cx));
		}
		cx.notify();
	}

	pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
		if self.connected != connected {
			self.connected = connected;
			cx.notify();
		}
	}

	pub fn show(
		&mut self,
		guild: Option<Snowflake>,
		channel: Option<Snowflake>,
		name: SharedString,
		direct: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.channel == channel && self.guild == guild {
			self.welcome = Rc::new(Welcome { name, direct });
			cx.notify();
			return;
		}
		if let Some(playback) = cx.try_global::<Playback>() {
			playback.stop_all();
		}
		self.guild = guild;
		self.channel = channel;
		self.welcome = Rc::new(Welcome { name, direct });
		self.editing = None;
		self.selection = Rc::default();
		self.requested_older = None;
		self.requested_newer = None;
		self.detached = false;
		self.pending_jump = None;
		self.flash = None;
		self.flash_reset = None;
		self.revealed.borrow_mut().clear();
		self.poll_picks.borrow_mut().clear();
		self.rows = Rc::new(Vec::new());
		self.drive(cx, |scroll, cx| scroll.reset(0, cx));
		self.refresh(window, cx);
	}

	pub fn on_changes(&mut self, changes: &Changes, window: &mut Window, cx: &mut Context<Self>) {
		if self
			.channel
			.is_some_and(|channel| changes.messages.contains(channel))
		{
			self.refresh(window, cx);
		}
		if changes.loaded.contains(state::EXPRESSIONS_KEY) {
			cx.notify();
		}
	}

	pub fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.refresh(window, cx);
		cx.notify();
	}

	fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let (rows, detached) = {
			let data = self.data.read(cx);
			let state = data.state();
			(
				state.channel_rows(channel),
				state.messages(channel).is_some_and(|cache| cache.is_detached()),
			)
		};
		let flipped = self.detached != detached;
		self.detached = detached;
		let old = &self.rows;
		let prefix = old.iter().zip(&rows).take_while(|(a, b)| same_row(a, b)).count();
		let tail = old.len().min(rows.len()) - prefix;
		let suffix = old
			.iter()
			.rev()
			.zip(rows.iter().rev())
			.take(tail)
			.take_while(|(a, b)| same_row(a, b))
			.count();
		let removed = prefix..old.len() - suffix;
		let added = rows.len() - prefix - suffix;
		let changed = !removed.is_empty() || added > 0;
		self.rows = Rc::new(rows);
		if changed {
			self.drive(cx, |scroll, cx| {
				scroll.splice(removed, added, cx);
			});
		}
		self.finish_jump(cx);
		self.acknowledge(channel, window, cx);
		if changed || flipped {
			cx.notify();
		}
	}

	fn acknowledge(&mut self, channel: Snowflake, window: &Window, cx: &mut Context<Self>) {
		if !window.is_window_active() || !self.scroll.read(cx).is_following_tail() {
			return;
		}
		let newest = self
			.data
			.read(cx)
			.state()
			.messages(channel)
			.and_then(|cache| cache.newest_id());
		if let Some(message_id) = newest {
			self.data.read(cx).send(Command::AckChannel {
				channel_id: channel,
				message_id,
			});
		}
	}

	fn first_message(&self) -> Option<Snowflake> {
		self.rows.iter().find_map(|row| match row {
			MessageRow::Message(view) if !view.pending => Some(view.message.id),
			_ => None,
		})
	}

	fn load_newer(&mut self, channel: Snowflake, cx: &mut Context<Self>) {
		let newest = self
			.data
			.read(cx)
			.state()
			.messages(channel)
			.and_then(|cache| cache.newest_id());
		let Some(newest) = newest else {
			return;
		};
		let stale = self
			.requested_newer
			.is_none_or(|(id, at)| id != newest || at.elapsed() > OLDER_RETRY);
		if stale {
			self.requested_newer = Some((newest, Instant::now()));
			self.data
				.read(cx)
				.send(Command::LoadNewer { channel_id: channel });
		}
	}

	fn jump_to_present(&mut self, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		self.drive(cx, |scroll, cx| scroll.scroll_to_end(cx));
		self.requested_newer = None;
		self.load_newer(channel, cx);
	}

	fn drive(
		&mut self,
		cx: &mut Context<Self>,
		apply: impl FnOnce(&mut ChatListState, &mut Context<ChatListState>),
	) {
		self.own_update = true;
		self.scroll.update(cx, apply);
	}

	fn snap_when_stuck(&mut self, cx: &mut Context<Self>) -> bool {
		if std::mem::take(&mut self.own_update) {
			self.scroll_anchor = None;
			return false;
		}
		let scroll = self.scroll.read(cx);
		let position = scroll.logical_scroll_top();
		let moved = position.item_ix > 0 || position.offset_in_item > px(0.);
		let stuck = moved
			&& self.scroll_anchor.is_some_and(|anchor| {
				anchor.item_ix == position.item_ix && anchor.offset_in_item == position.offset_in_item
			});
		self.scroll_anchor = Some(position);
		if stuck && scroll.is_scrolled_up() {
			self.scroll_to_end(cx);
			return true;
		}
		false
	}

	fn on_scroll(&mut self, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		if self.snap_when_stuck(cx) {
			return;
		}
		if self.detached && !self.scroll.read(cx).is_scrolled_up() {
			self.load_newer(channel, cx);
		}
		if !matches!(self.rows.first(), Some(MessageRow::Loading)) {
			return;
		}
		let scroll = self.scroll.read(cx);
		let near_top = scroll.is_scrolled_up() && scroll.logical_scroll_top().item_ix < NEAR_TOP_ROWS;
		let short = self.rows.len() < SHORT_LIST_ROWS;
		if !near_top && !short {
			return;
		}
		let Some(first) = self.first_message() else {
			return;
		};
		let stale = self
			.requested_older
			.is_none_or(|(id, at)| id != first || at.elapsed() > OLDER_RETRY);
		if stale {
			self.requested_older = Some((first, Instant::now()));
			self.data
				.read(cx)
				.send(Command::LoadOlder { channel_id: channel });
		}
	}

	pub fn jump_to(&mut self, message_id: Snowflake, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		if self.scroll_to(message_id, cx) {
			return;
		}
		self.pending_jump = Some(message_id);
		self.data.read(cx).send(Command::LoadAround {
			channel_id: channel,
			message_id,
		});
	}

	fn scroll_to(&mut self, message_id: Snowflake, cx: &mut Context<Self>) -> bool {
		let index = self.rows.iter().position(|row| match row {
			MessageRow::Message(view) => view.message.id == message_id,
			_ => false,
		});
		let Some(index) = index else {
			return false;
		};
		let head = self.group_head(index);
		self.own_update = true;
		let scrolled = self
			.scroll
			.update(cx, |scroll, cx| scroll.scroll_to_item(head, cx));
		if scrolled {
			self.flash_message(message_id, cx);
		}
		scrolled
	}

	fn group_head(&self, index: usize) -> usize {
		let floor = index.saturating_sub(GROUP_LOOKBACK);
		(floor..index)
			.rev()
			.take_while(
				|ix| matches!(&self.rows[ix + 1], MessageRow::Message(view) if view.grouped_with_previous),
			)
			.last()
			.unwrap_or(index)
	}

	fn flash_message(&mut self, message_id: Snowflake, cx: &mut Context<Self>) {
		self.flash = Some(message_id);
		self.flash_reset = Some(cx.spawn(async move |this, cx| {
			cx.background_executor().timer(FLASH_DURATION).await;
			this.update(cx, |this, cx| {
				this.flash = None;
				cx.notify();
			})
			.ok();
		}));
		cx.notify();
	}

	fn finish_jump(&mut self, cx: &mut Context<Self>) {
		if let Some(message_id) = self.pending_jump
			&& self.scroll_to(message_id, cx)
		{
			self.pending_jump = None;
		}
	}

	fn link_origin(&self, message: Option<Snowflake>) -> LinkOrigin {
		LinkOrigin {
			guild: self.guild,
			channel: self.channel,
			message,
		}
	}

	fn on_row_action(&mut self, action: RowAction, window: &mut Window, cx: &mut Context<Self>) {
		match action {
			RowAction::OpenLink(url) | RowAction::Download(url) => {
				let origin = self.link_origin(None);
				cx.emit(TimelineEvent::OpenLink(url, origin));
			}
			RowAction::OpenMessageLink { url, message_id } => {
				let origin = self.link_origin(Some(Snowflake(message_id)));
				cx.emit(TimelineEvent::OpenLink(url, origin));
			}
			RowAction::ToggleSpoiler(message, index) => {
				if let (Some(channel), Ok(ordinal)) = (self.channel, u32::try_from(index))
					&& plugins::message_click_here(
						(self.guild, channel, Snowflake(message)),
						ClickTarget::Spoiler(ordinal),
						window,
						cx,
					) {
					return;
				}
				self.revealed.borrow_mut().insert((message, index));
				cx.notify();
			}
			RowAction::OpenImage(target) => cx.emit(TimelineEvent::OpenImage(target)),
			RowAction::ToggleGifFavorite(gif) => {
				let data = self.data.read(cx);
				let favorited = data
					.state()
					.gif_favorites()
					.map(|favorites| favorites.iter().any(|known| known.url == gif.url));
				match favorited {
					Some(true) => data.send(Command::UnfavoriteGif { url: gif.url }),
					Some(false) => data.send(Command::FavoriteGif { gif }),
					None => {
						data.send(Command::LoadGifFavorites);
						data.send(Command::FavoriteGif { gif });
					}
				}
			}
			RowAction::React {
				channel_id,
				message_id,
				emoji,
				add,
			} => {
				let command = if add {
					Command::AddReaction {
						channel_id,
						message_id,
						emoji,
						burst: plugins::client_options(cx).super_react_default.unwrap_or(false),
					}
				} else {
					Command::RemoveReaction {
						channel_id,
						message_id,
						emoji,
					}
				};
				self.data.read(cx).send(command);
			}
			RowAction::Reply(message) => cx.emit(TimelineEvent::Reply(message)),
			RowAction::CreateThread(message) => {
				if let Some(channel) = self.channel {
					act(window, cx, move |community, window, cx| {
						community.create_thread(channel, Some(message), window, cx)
					});
				}
			}
			RowAction::OpenThread(thread) => act(window, cx, move |community, _, cx| {
				community.open_thread(thread, cx)
			}),
			RowAction::Pin { message, pin } => {
				if let Some(channel) = self.channel {
					act(window, cx, move |community, window, cx| {
						community.confirm_pin(channel, message, pin, window, cx)
					});
				}
			}
			RowAction::MarkUnread(message_id) => {
				if let Some(channel_id) = self.channel {
					self.data.read(cx).send(Command::MarkUnread {
						channel_id,
						message_id,
					});
				}
			}
			RowAction::Forward(message) => {
				if let Some(channel) = self.channel {
					act(window, cx, move |community, window, cx| {
						community.open_forward(channel, message, window, cx)
					});
				}
			}
			RowAction::PollVote {
				channel_id,
				message_id,
				answer_ids,
			} => {
				self.poll_picks.borrow_mut().remove(&message_id);
				self.data.read(cx).send(Command::VotePoll {
					channel_id,
					message_id,
					answer_ids,
				});
				self.remeasure(message_id, cx);
				cx.notify();
			}
			RowAction::PollToggle {
				message_id,
				answer_id,
				mut current,
			} => {
				match current.iter().position(|id| *id == answer_id) {
					Some(index) => {
						current.remove(index);
					}
					None => current.push(answer_id),
				}
				self.poll_picks.borrow_mut().insert(message_id, current);
				self.remeasure(message_id, cx);
				cx.notify();
			}
			RowAction::PollEnd {
				channel_id,
				message_id,
			} => self.data.read(cx).send(Command::EndPoll {
				channel_id,
				message_id,
			}),
			RowAction::UseComponent(used) => self.data.read(cx).send(Command::UseComponent(used)),
			RowAction::RunCommand { message, command } => {
				if let Some(channel_id) = self.channel {
					self.data.read(cx).send(Command::RunMessageCommand {
						guild_id: self.guild,
						channel_id,
						message_id: message,
						command,
					});
				}
			}
			RowAction::Save { message, due } => {
				if let Some(channel_id) = self.channel {
					self.data.read(cx).send(Command::SaveMessage {
						channel_id,
						message_id: message,
						due_at: due,
					});
				}
			}
			RowAction::Publish(message_id) => {
				if let Some(channel_id) = self.channel {
					self.data.read(cx).send(Command::PublishMessage {
						channel_id,
						message_id,
					});
				}
			}
			RowAction::Select(message) => {
				let mut selection = (*self.selection).clone();
				if !selection.remove(&message) {
					selection.insert(message);
				}
				self.selection = Rc::new(selection);
				cx.notify();
			}
			RowAction::DeleteSelected => self.delete_selected(window, cx),
			RowAction::Report => moderation::open_report(),
			RowAction::StartEdit(message) => self.start_edit(message, window, cx),
			RowAction::Delete(message) => self.confirm_delete(message, window, cx),
			RowAction::Resend {
				channel_id,
				content,
				reply_to,
			} => self.data.read(cx).send(Command::SendMessage {
				channel_id,
				content,
				reply_to: reply_to.map(|message| ReplyTarget {
					message,
					mention: true,
				}),
				files: Vec::new(),
				stickers: Vec::new(),
				by_plugin: None,
			}),
			RowAction::OpenEmoji { message_id, at } => cx.emit(TimelineEvent::OpenEmoji { message_id, at }),
			RowAction::OpenProfile { user, at } => cx.emit(TimelineEvent::OpenProfile { user, at }),
			RowAction::JumpTo(message) => self.jump_to(message, cx),
		}
	}

	fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let mut ids: Vec<Snowflake> = self.selection.iter().copied().collect();
		ids.sort_unstable();
		self.selection = Rc::default();
		moderation::dialogs::open_bulk_delete(window, cx, channel, ids);
		cx.notify();
	}

	fn access(&self, cx: &Context<Self>) -> MessageAccess {
		let Some(channel) = self.channel else {
			return MessageAccess::default();
		};
		let data = self.data.read(cx);
		let state = data.state();
		let permissions = state.my_permissions(channel);
		let kind = state.channel(channel).map(|found| found.kind);
		let direct = self.guild.is_none();
		MessageAccess {
			send: direct || state.can_send(channel),
			react: direct || state.can_react(channel),
			read_history: direct || permissions.contains(protocol::Permissions::READ_MESSAGE_HISTORY),
			manage_messages: self.guild.is_some() && state.can_manage_messages(channel),
			pin: direct || state.can_pin(channel),
			create_threads: self.guild.is_some()
				&& matches!(
					kind,
					Some(ChannelType::GuildText | ChannelType::GuildAnnouncement)
				) && permissions.contains(
				protocol::Permissions::CREATE_PUBLIC_THREADS | protocol::Permissions::READ_MESSAGE_HISTORY,
			),
			in_thread: matches!(
				kind,
				Some(
					ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::AnnouncementThread
				)
			),
			announcement: kind == Some(ChannelType::GuildAnnouncement),
		}
	}

	fn selection_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.flex()
			.flex_none()
			.items_center()
			.justify_between()
			.gap(theme.space_12)
			.mx(theme.space_16)
			.mb(theme.space_8)
			.px(theme.space_16)
			.py(theme.space_8)
			.rounded(theme.radius_md)
			.bg(theme.background_floating)
			.border_1()
			.border_color(theme.divider)
			.child(
				div()
					.text_size(theme.font_md)
					.text_color(theme.text_normal)
					.child(cx.t_args(
						"timeline-selected",
						&[("count", crate::i18n::ArgValue::from(self.selection.len()))],
					)),
			)
			.child(
				div()
					.flex()
					.items_center()
					.gap(theme.space_8)
					.child(
						Button::new("selection-clear")
							.ghost()
							.label(cx.t("action-cancel"))
							.on_click(cx.listener(|this, _, _, cx| {
								this.selection = Rc::default();
								cx.notify();
							})),
					)
					.child(
						Button::new("selection-delete")
							.danger()
							.label(cx.t("timeline-delete-selected"))
							.on_click(cx.listener(|this, _, window, cx| this.delete_selected(window, cx))),
					),
			)
	}

	fn row_index(&self, message_id: Snowflake) -> Option<usize> {
		self.rows.iter().position(|row| match row {
			MessageRow::Message(view) => view.message.id == message_id,
			_ => false,
		})
	}

	fn remeasure(&mut self, message_id: Snowflake, cx: &mut Context<Self>) {
		if let Some(index) = self.row_index(message_id) {
			self.drive(cx, |scroll, cx| {
				scroll.remeasure_items(index..index + 1, cx);
			});
		}
	}

	pub fn stop_editing(&mut self, cx: &mut Context<Self>) -> bool {
		let Some(editing) = self.editing.take() else {
			return false;
		};
		self.remeasure(editing.message, cx);
		cx.notify();
		true
	}

	pub fn begin_edit(&mut self, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		self.start_edit(message_id, window, cx);
	}

	pub fn jump_to_top(&mut self, cx: &mut Context<Self>) {
		self.drive(cx, |scroll, cx| {
			scroll.scroll_to_item(0, cx);
		});
	}

	fn start_edit(&mut self, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let content = {
			let data = self.data.read(cx);
			let state = data.state();
			state
				.messages(channel)
				.and_then(|cache| cache.by_id.get(&message_id))
				.map(|message| message.content.clone())
		};
		let Some(content) = content else {
			return;
		};
		if let Some(previous) = self.editing.take() {
			self.remeasure(previous.message, cx);
		}
		let input = cx.new(|cx| {
			TextareaState::new(window, cx)
				.auto_grow(1, 10)
				.submit_on_enter(true)
		});
		let content = {
			let data = self.data.read(cx);
			let snapshot = data.state();
			let lookup = Lookup {
				state: &snapshot,
				guild: self.guild,
			};
			inline_emoji::content(content, &lookup)
		};
		input.update(cx, |state, cx| {
			state.set_value(content, window, cx);
			state.focus(window, cx);
		});
		let subscription = cx.subscribe_in(
			&input,
			window,
			move |this, input, event: &InputEvent, window, cx| {
				if let InputEvent::Change = event {
					if let Some(editing) = &mut this.editing {
						editing.tokens.sync(input, &this.data, this.guild, window, cx);
					}
					return;
				}
				let InputEvent::PressEnter {
					secondary: false,
					shift: false,
				} = event
				else {
					return;
				};
				let text = input.read(cx).value().trim().to_owned();
				if !text.is_empty() {
					this.data.read(cx).send(Command::EditMessage {
						channel_id: channel,
						message_id,
						content: text,
					});
				}
				this.stop_editing(cx);
			},
		);
		self._subscriptions.push(subscription);
		self.editing = Some(Editing {
			message: message_id,
			input,
			tokens: EmojiTokens::default(),
		});
		self.remeasure(message_id, cx);
		cx.notify();
	}

	fn confirm_delete(&mut self, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let data = self.data.clone();
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			alert
				.title(cx.t("delete-title"))
				.description(cx.t("delete-body"))
				.confirm()
				.ok_text(cx.t("menu-delete"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::DeleteMessage {
						channel_id: channel,
						message_id,
					});
					true
				})
		});
	}

	fn dispatcher(&self, cx: &Context<Self>) -> Dispatch {
		let weak = cx.weak_entity();
		Rc::new(move |action, window, cx| {
			weak.update(cx, |timeline, cx| timeline.on_row_action(action, window, cx))
				.ok();
		})
	}

	fn present_pill(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.absolute()
			.bottom(theme.space_16)
			.left_0()
			.right_0()
			.flex()
			.justify_center()
			.child(
				Button::new("timeline-jump-present")
					.primary()
					.rounded(theme.pixels(theme.radius_md))
					.icon(gpui_component::Icon::default().path(IconKind::ArrowDown.path()))
					.label(cx.t("timeline-jump-present"))
					.on_click(cx.listener(|this, _, _, cx| this.jump_to_present(cx))),
			)
	}

	fn banner(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
		let data = self.data.clone();
		div()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.gap(theme.space_12)
			.h(rem(32.))
			.bg(theme.warning)
			.text_size(theme.font_sm)
			.font_weight(FontWeight::SEMIBOLD)
			.text_color(theme.background_tertiary)
			.child(cx.t("connection-reconnecting"))
			.child(
				Button::new("connection-retry")
					.xsmall()
					.outline()
					.label(cx.t("connection-retry"))
					.on_click(move |_: &ClickEvent, _, cx| data.read(cx).send(Command::Reconnect)),
			)
	}
}

impl Render for Timeline {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		Theme::ensure_code_shift(window, cx);
		let theme = Theme::current(cx);
		let data = self.data.clone();
		let dispatch = self.dispatcher(cx);
		let rows = self.rows.clone();
		let welcome = self.welcome.clone();
		let revealed = self.revealed.clone();
		let poll_picks = self.poll_picks.clone();
		let prefs = Prefs::load(cx);
		let detached = self.detached;
		let documents = self.documents.clone();
		let editing = self.editing.clone();
		let compact = self.compact;
		let guild = self.guild;
		let access = self.access(cx);
		let selection = self.selection.clone();
		let flash = self.flash;
		let row_theme = theme.clone();
		let jump_label = (!detached).then(|| cx.t("timeline-jump-latest"));
		let scroller = ChatList::new("timeline", self.scroll.clone())
			.jump_button(jump_label)
			.render(
				move |index, _, cx| {
					let Some(row) = rows.get(index) else {
						return div().into_any_element();
					};
					let data = data.read(cx);
					let state = data.state();
					let lookup = Lookup { state: &state, guild };
					let revealed = revealed.borrow();
					let poll_picks = poll_picks.borrow();
					let env = RowEnv {
						theme: &row_theme,
						lookup: &lookup,
						dispatch: &dispatch,
						revealed: &revealed,
						poll_picks: &poll_picks,
						prefs,
						documents: &documents,
						me: state.current_user_id(),
						compact,
						editing: editing.as_ref(),
						welcome: &welcome,
						access,
						selection: &selection,
						flash,
						options: *state.client_options(),
					};
					render_row(row, &env, cx)
				},
				&*cx,
			);
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.bg(theme.background_primary)
			.on_action(cx.listener(|this, _: &Escape, _, cx| {
				if !this.selection.is_empty() {
					this.selection = Rc::default();
					cx.notify();
				} else if !this.stop_editing(cx) {
					cx.propagate();
				}
			}))
			.when(!self.connected, |timeline| {
				timeline.child(self.banner(&theme, cx))
			})
			.child(
				div()
					.relative()
					.flex()
					.flex_col()
					.flex_1()
					.min_h_0()
					.child(scroller)
					.when(detached, |area| area.child(self.present_pill(&theme, cx))),
			)
			.when(!self.selection.is_empty(), |timeline| {
				timeline.child(self.selection_bar(&theme, cx))
			})
			.child(crate::playback::Playback::view_marker())
	}
}
