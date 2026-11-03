use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
	App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FontWeight, InteractiveElement as _,
	IntoElement, ListOffset, ParentElement as _, Pixels, Point, Render, SharedString,
	StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
	prelude::FluentBuilder as _, px,
};
use gpui_component::Sizable as _;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Escape, InputEvent, TextareaState};
use lru::LruCache;
use plugin_host::api::model::ClickTarget;
use protocol::{ChannelType, Snowflake};
use session::Command;
use state::{Changes, MessageRow, UnreadSince};

use super::community::{act, save_file};
use super::inline_emoji::{self, EmojiTokens};
use super::message::{
	Dispatch, DocumentCache, Editing, FLASH_DURATION, Gallery, GifFavoriteOverrides, MessageAccess, Prefs,
	RowAction, RowEnv, VideoFullscreenTarget, Welcome, can_edit, render_row,
};
use super::moderation;
use super::plugins::{self, LinkOrigin, PluginBridge};
use crate::components::{ChatList, ChatListState};
use crate::data::Data;
use crate::format;
use crate::i18n::{ArgValue, Translate as _};
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
	OpenImage(Gallery),
	OpenVideoFullscreen(VideoFullscreenTarget),
	Reply(Snowflake),
	OpenEmoji {
		message_id: Snowflake,
		at: Point<Pixels>,
	},
	OpenProfile {
		user: Snowflake,
		at: Point<Pixels>,
	},
	PrefillCommand(String),
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
	read_anchor: Option<Snowflake>,
	own_ack: Option<Snowflake>,
	anchor_resolved: bool,
	unread: Option<UnreadSince>,
	marker_index: Option<usize>,
	banner: bool,
	relative_tick: Option<Task<()>>,
	relative_seen: Rc<Cell<bool>>,
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
				&& x.retryable == y.retryable
				&& x.upload == y.upload
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
		(
			MessageRow::BlockedRun {
				first: x,
				last: x_last,
				count: x_count,
				revealed: x_revealed,
			},
			MessageRow::BlockedRun {
				first: y,
				last: y_last,
				count: y_count,
				revealed: y_revealed,
			},
		) => x == y && x_last == y_last && x_count == y_count && x_revealed == y_revealed,
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
			read_anchor: None,
			own_ack: None,
			anchor_resolved: false,
			unread: None,
			marker_index: None,
			banner: false,
			relative_tick: None,
			relative_seen: Rc::default(),
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
		self.requested_older = None;
		self.requested_newer = None;
		self.detached = false;
		self.pending_jump = None;
		self.flash = None;
		self.flash_reset = None;
		self.revealed.borrow_mut().clear();
		self.poll_picks.borrow_mut().clear();
		self.read_anchor = None;
		self.own_ack = None;
		self.anchor_resolved = false;
		self.unread = None;
		self.marker_index = None;
		self.banner = false;
		self.rows = Rc::new(Vec::new());
		self.drive(cx, |scroll, cx| scroll.reset(0, cx));
		self.refresh(window, cx);
	}

	pub fn on_changes(&mut self, changes: &Changes, window: &mut Window, cx: &mut Context<Self>) {
		if self.affected_by(changes, cx) {
			self.refresh(window, cx);
		}
		if changes.loaded.contains(state::EXPRESSIONS_KEY) {
			cx.default_global::<GifFavoriteOverrides>().0.clear();
			cx.notify();
		}
	}

	fn affected_by(&self, changes: &Changes, cx: &App) -> bool {
		let Some(channel) = self.channel else {
			return false;
		};
		changes.messages.contains(channel)
			|| self
				.data
				.read(cx)
				.state()
				.channel(channel)
				.and_then(|channel| channel.parent_id)
				.is_some_and(|parent| changes.messages.contains(parent))
	}

	pub fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.refresh(window, cx);
		cx.notify();
	}

	fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let (rows, detached, unread) = {
			let data = self.data.read(cx);
			let state = data.state();
			if let Some(anchor) = self.read_anchor {
				let acked = state.read_state(channel).and_then(|read| read.last_message_id);
				if acked.is_some_and(|id| id > anchor && self.own_ack.is_none_or(|own| id > own)) {
					self.read_anchor = None;
				}
			}
			if !self.anchor_resolved && state.messages(channel).is_some() {
				self.anchor_resolved = true;
				self.read_anchor = state
					.read_state(channel)
					.and_then(|read| read.last_message_id)
					.filter(|anchor| state.unread_since(channel, *anchor).is_some());
			}
			(
				state.channel_rows_after(channel, self.read_anchor),
				state.messages(channel).is_some_and(|cache| cache.is_detached()),
				self.read_anchor
					.and_then(|anchor| state.unread_since(channel, anchor)),
			)
		};
		self.unread = unread;
		self.marker_index = rows
			.iter()
			.position(|row| matches!(row, MessageRow::UnreadMarker));
		let flipped = self.detached != detached;
		self.detached = detached;
		let anchor = self.top_message(cx);
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
			self.restore_top(anchor, cx);
		}
		self.finish_jump(cx);
		self.acknowledge(channel, window, cx);
		if changed || flipped || self.sync_banner(cx) {
			cx.notify();
		}
	}

	fn sync_banner(&mut self, cx: &Context<Self>) -> bool {
		let top = self.scroll.read(cx).logical_scroll_top().item_ix;
		let banner = self.unread.is_some() && self.marker_index.is_none_or(|marker| marker < top);
		std::mem::replace(&mut self.banner, banner) != banner
	}

	fn mark_read(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		self.data
			.read(cx)
			.send(Command::MarkChannelRead { channel_id: channel });
		self.read_anchor = None;
		self.refresh(window, cx);
	}

	fn schedule_relative_refresh(&mut self, cx: &mut Context<Self>) {
		if self.relative_tick.is_some() || !self.relative_seen.take() {
			return;
		}
		self.relative_tick = Some(cx.spawn(async move |this, cx| {
			cx.background_executor().timer(format::until_next_minute()).await;
			this.update(cx, |this, cx| {
				this.relative_tick = None;
				cx.notify();
			})
			.ok();
		}));
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
			self.own_ack = Some(message_id);
			self.data.read(cx).send(Command::AckChannel {
				channel_id: channel,
				message_id,
			});
		}
	}

	fn first_message(&self) -> Option<Snowflake> {
		self.rows.iter().find_map(|row| match row {
			MessageRow::Message(view) if !view.pending => Some(view.message.id),
			MessageRow::BlockedRun { first, .. } => Some(*first),
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
		if !self.detached {
			return;
		}
		let newest = self
			.data
			.read(cx)
			.state()
			.messages(channel)
			.and_then(|cache| cache.newest_id());
		self.requested_newer = newest.map(|newest| (newest, Instant::now()));
		self.data
			.read(cx)
			.send(Command::LoadLatest { channel_id: channel });
	}

	fn top_message(&self, cx: &Context<Self>) -> Option<(Snowflake, Pixels)> {
		if self.pending_jump.is_some() {
			return None;
		}
		let scroll = self.scroll.read(cx);
		if scroll.is_following_tail() {
			return None;
		}
		let top = scroll.logical_scroll_top();
		match self.rows.get(top.item_ix) {
			Some(MessageRow::Message(view)) => Some((view.message.id, top.offset_in_item)),
			Some(MessageRow::BlockedRun { first, .. }) => Some((*first, top.offset_in_item)),
			_ => None,
		}
	}

	fn restore_top(&mut self, top: Option<(Snowflake, Pixels)>, cx: &mut Context<Self>) {
		let Some((message, offset)) = top else {
			return;
		};
		let Some(index) = self.row_index(message) else {
			return;
		};
		let current = self.scroll.read(cx).logical_scroll_top();
		if current.item_ix == index && current.offset_in_item == offset {
			return;
		}
		self.drive(cx, |scroll, cx| {
			scroll.scroll_to(index, offset, cx);
		});
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
		if self.sync_banner(cx) {
			cx.notify();
		}
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
		let Some(index) = self.row_index(message_id) else {
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
			RowAction::OpenLink(url) => {
				let origin = self.link_origin(None);
				cx.emit(TimelineEvent::OpenLink(url, origin));
			}
			RowAction::Download { url, name } => save_file(&url, name, cx),
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
			RowAction::ToggleBlocked(first) => {
				if let Some(channel_id) = self.channel {
					self.data
						.read(cx)
						.send(Command::ToggleBlockedRun { channel_id, first });
				}
			}
			RowAction::OpenImage(target) => cx.emit(TimelineEvent::OpenImage(target)),
			RowAction::OpenVideoFullscreen(target) => cx.emit(TimelineEvent::OpenVideoFullscreen(target)),
			RowAction::ToggleGifFavorite(gif) => {
				let favorited = cx
					.try_global::<GifFavoriteOverrides>()
					.and_then(|overrides| overrides.0.get(&gif.url).copied())
					.unwrap_or_else(|| {
						self.data
							.read(cx)
							.state()
							.gif_favorites()
							.is_some_and(|favorites| favorites.iter().any(|known| known.url == gif.url))
					});
				cx.default_global::<GifFavoriteOverrides>()
					.0
					.insert(gif.url.clone(), !favorited);
				let data = self.data.read(cx);
				if favorited {
					data.send(Command::UnfavoriteGif { url: gif.url });
				} else {
					data.send(Command::FavoriteGif { gif });
				}
				cx.notify();
			}
			RowAction::React {
				channel_id,
				message_id,
				emoji,
				add,
				burst,
			} => {
				let command = if add {
					Command::AddReaction {
						channel_id,
						message_id,
						emoji,
						burst,
					}
				} else {
					Command::RemoveReaction {
						channel_id,
						message_id,
						emoji,
						burst,
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
			RowAction::Report => moderation::open_report(),
			RowAction::StartEdit(message) => self.start_edit(message, window, cx),
			RowAction::Delete(message) => self.confirm_delete(message, window, cx),
			RowAction::Retry(nonce) => self.data.read(cx).send(Command::RetryPending { nonce }),
			RowAction::Discard(nonce) => self.data.read(cx).send(Command::DiscardPending { nonce }),
			RowAction::SuppressEmbeds(message) => self.confirm_suppress(message, window, cx),
			RowAction::RemoveEditAttachment { message, attachment } => {
				if let Some(editing) = self.editing.as_mut().filter(|editing| editing.message == message) {
					editing.removed.insert(attachment);
					self.remeasure(message, cx);
					cx.notify();
				}
			}
			RowAction::OpenEmoji { message_id, at } => cx.emit(TimelineEvent::OpenEmoji { message_id, at }),
			RowAction::OpenProfile { user, at } => cx.emit(TimelineEvent::OpenProfile { user, at }),
			RowAction::JumpTo(message) => self.jump_to(message, cx),
			RowAction::OpenChannel(channel_id) => {
				let data = self.data.read(cx);
				let guild_id = data.state().channel_guild(channel_id);
				data.send(Command::SelectChannel { guild_id, channel_id });
			}
			RowAction::PrefillCommand(name) => cx.emit(TimelineEvent::PrefillCommand(name)),
		}
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
			pin: state.can_pin(channel),
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

	fn row_index(&self, message_id: Snowflake) -> Option<usize> {
		self.rows.iter().position(|row| match row {
			MessageRow::Message(view) => view.message.id == message_id,
			MessageRow::BlockedRun { first, last, .. } => (*first..=*last).contains(&message_id),
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

	pub fn edit_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let last = {
			let data = self.data.read(cx);
			let state = data.state();
			let me = state.current_user_id();
			state.messages(channel).and_then(|cache| {
				cache
					.ordered
					.iter()
					.rev()
					.find(|message| !cache.is_deleted(message.id) && can_edit(message, me))
					.map(|message| message.id)
			})
		};
		if let Some(message_id) = last {
			self.jump_to(message_id, cx);
			self.start_edit(message_id, window, cx);
		}
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
				let keep = this.kept_attachments(channel, message_id, cx);
				if !text.is_empty() || keep.as_ref().is_some_and(|keep| !keep.is_empty()) {
					this.data.read(cx).send(Command::EditMessage {
						channel_id: channel,
						message_id,
						content: text,
						keep,
					});
				} else if keep.is_some() || this.is_text_only(message_id, cx) {
					this.confirm_delete(message_id, window, cx);
				}
				this.stop_editing(cx);
			},
		);
		self._subscriptions.push(subscription);
		self.editing = Some(Editing {
			message: message_id,
			input,
			tokens: EmojiTokens::default(),
			removed: HashSet::new(),
		});
		self.remeasure(message_id, cx);
		cx.notify();
	}

	fn is_text_only(&self, message_id: Snowflake, cx: &Context<Self>) -> bool {
		let Some(channel) = self.channel else {
			return false;
		};
		let data = self.data.read(cx);
		let state = data.state();
		state
			.messages(channel)
			.and_then(|cache| cache.by_id.get(&message_id))
			.is_some_and(|message| message.attachments.is_empty() && message.sticker_items.is_empty())
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

	fn kept_attachments(
		&self,
		channel: Snowflake,
		message_id: Snowflake,
		cx: &Context<Self>,
	) -> Option<Vec<Snowflake>> {
		let removed = &self.editing.as_ref()?.removed;
		if removed.is_empty() {
			return None;
		}
		let data = self.data.read(cx);
		let state = data.state();
		let message = state.messages(channel)?.by_id.get(&message_id)?;
		let kept = message
			.attachments
			.iter()
			.map(|attachment| attachment.id)
			.filter(|id| !removed.contains(id))
			.collect();
		Some(kept)
	}

	fn confirm_suppress(&mut self, message_id: Snowflake, window: &mut Window, cx: &mut Context<Self>) {
		let Some(channel) = self.channel else {
			return;
		};
		let data = self.data.clone();
		window.open_alert_dialog(cx, move |alert, _, cx| {
			let data = data.clone();
			alert
				.title(cx.t("suppress-title"))
				.description(cx.t("suppress-body"))
				.confirm()
				.ok_text(cx.t("suppress-confirm"))
				.ok_variant(ButtonVariant::Danger)
				.cancel_text(cx.t("action-cancel"))
				.on_ok(move |_, _, cx| {
					data.read(cx).send(Command::SuppressEmbeds {
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

	fn unread_bar(&self, unread: UnreadSince, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
		let key = if unread.more {
			"timeline-unread-more"
		} else {
			"timeline-unread"
		};
		let label = cx.t_args(
			key,
			&[
				("count", ArgValue::from(unread.count)),
				(
					"time",
					ArgValue::from(format::message_time(unread.since, cx).to_string()),
				),
			],
		);
		div()
			.absolute()
			.top_0()
			.left_0()
			.right_0()
			.flex()
			.items_center()
			.justify_between()
			.gap(theme.space_12)
			.h(rem(32.))
			.px(theme.space_16)
			.bg(theme.brand)
			.text_size(theme.font_sm)
			.font_weight(FontWeight::SEMIBOLD)
			.text_color(theme.text_on_brand)
			.child(
				div()
					.id("timeline-unread-jump")
					.flex_1()
					.min_w_0()
					.truncate()
					.cursor_pointer()
					.on_click(cx.listener(move |this, _, _, cx| this.jump_to(unread.jump_to, cx)))
					.child(label),
			)
			.child(
				div()
					.id("timeline-mark-read")
					.flex_none()
					.cursor_pointer()
					.on_click(cx.listener(|this, _, window, cx| this.mark_read(window, cx)))
					.child(cx.t("menu-mark-as-read")),
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
		if self.relative_tick.is_none() {
			cx.on_next_frame(window, |this, _, cx| this.schedule_relative_refresh(cx));
		}
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
		let relative_seen = self.relative_seen.clone();
		let flash = self.flash;
		let row_theme = theme.clone();
		let jump_label = (!detached).then(|| cx.t("timeline-jump-latest"));
		let jump_target = cx.weak_entity();
		let scroller = ChatList::new("timeline", self.scroll.clone())
			.jump_button(jump_label)
			.on_jump(move |_, cx| {
				jump_target
					.update(cx, |timeline, cx| timeline.jump_to_present(cx))
					.ok();
			})
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
						flash,
						options: *state.client_options(),
						relative_seen: &relative_seen,
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
				if !this.stop_editing(cx) {
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
					.when_some(self.unread.filter(|_| self.banner), |area, unread| {
						area.child(self.unread_bar(unread, &theme, cx))
					})
					.when(detached, |area| area.child(self.present_pill(&theme, cx))),
			)
			.child(crate::playback::Playback::view_marker())
	}
}
